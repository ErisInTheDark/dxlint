//! The engine's side of [`crate::parts`]: describing a unit's version from
//! its tree and names, and judging part reads against a file as it reads now.

use std::sync::{Arc, Mutex};

use crate::fx_hash::FxHashMap;
use crate::names::Resolved;
use crate::parts::{name_hash, Diff, FileParts, Member, Name, Part, PartReads};

use super::reads::{self, DepPath};
use super::units::Unit;
use super::Linter;

/// Versions of files as they read now, and how older versions differ from them.
#[derive(Default)]
pub(super) struct Judged {
    now: Mutex<FxHashMap<DepPath, Arc<FileParts>>>,
    diffs: Mutex<FxHashMap<(DepPath, u64, u64), Arc<Diff>>>,
    /// Each file's text hash, as of an invalidation revision: files change
    /// only through invalidations, and many part reads name one file.
    hashes: Mutex<FxHashMap<DepPath, (u64, u64)>>,
}

/// Diffs kept at most: each is small, but a long session judges many.
const DIFFS_KEPT: usize = 4096;

impl Linter {
    /// Whether part `read` of `parts` (an earlier version of `path`) reads
    /// the same in the file as it reads now.
    pub(super) fn part_holds(&self, path: &DepPath, parts: &Arc<FileParts>, read: &PartReads, strict: bool) -> bool {
        let hash = self.text_hash(path);
        if hash == parts.hash { return true }
        let Some(now) = self.parts_now(path, hash) else { return false };
        let key = (path.clone(), parts.hash, now.hash);
        let found = self.parts.diffs.lock().expect("part diffs").get(&key).cloned();
        let diff = found.unwrap_or_else(|| {
            let diff = Arc::new(parts.diff(&now));
            let mut diffs = self.parts.diffs.lock().expect("part diffs");
            if diffs.len() >= DIFFS_KEPT { diffs.clear() }
            diffs.insert(key, diff.clone());
            diff
        });
        parts.holds_all(&diff, read, strict)
    }

    /// `path`'s text hash now.
    fn text_hash(&self, path: &DepPath) -> u64 {
        let revision = self.invalidation.load(std::sync::atomic::Ordering::Acquire);
        if let Some(&(at, hash)) = self.parts.hashes.lock().expect("part hashes").get(path) {
            if at == revision { return hash }
        }
        let (_, hash) = reads::read_unrecorded(path);
        self.parts.hashes.lock().expect("part hashes").insert(path.clone(), (revision, hash));
        hash
    }

    /// `path`'s parts as its text (hashing `hash`) reads now; `None` when
    /// it does not parse.
    pub(super) fn parts_now(&self, path: &DepPath, hash: u64) -> Option<Arc<FileParts>> {
        if let Some(found) = self.parts.now.lock().expect("parts now").get(path).filter(|parts| parts.hash == hash) {
            return Some(found.clone());
        }
        let parts = self.alone_unit(path, |world, unit| Arc::new(super::world::describe_alone(world, unit)))?;
        self.parts.now.lock().expect("parts now").insert(path.clone(), parts.clone());
        Some(parts)
    }

    /// The opaque body of `unit` holding byte `at`, if any.
    pub(super) fn body_index(&self, unit: &Unit, at: u32) -> Option<u32> {
        self.bodies(unit)?.body_at(at).map(|index| index as u32)
    }

    /// `unit`'s parts, given its resolved names (`None`: unknown, so every
    /// read of it is of the whole file) and what its top-level scope
    /// publishes, by name (`None`: a computed publication).
    pub(super) fn describe(&self, unit: &Unit, names: Option<&Resolved>, published: &[(Option<String>, u32)]) -> FileParts {
        let tree = &unit.parsed.tree;
        let source = &*unit.text;
        let text = source.as_bytes();
        let slice = |start: u32, end: u32| &text[(start as usize).min(text.len())..(end as usize).min(text.len())];
        let coarse = self.bodies(unit);
        let body_spans: Vec<(u32, u32)> = coarse.as_ref().map(|bodies| bodies.spans().collect()).unwrap_or_default();
        let body_at = |at: u32| {
            let after = body_spans.partition_point(|&(start, _)| start <= at);
            (after > 0 && at < body_spans[after - 1].1).then(|| after - 1)
        };
        // Statements: the top level's children (below wrappers holding the
        // whole file), each with the text before it. Layers grafted beside
        // them (comments, line endings) are no statements.
        let syntax = |node: u32| !tree.layer_roots.iter().any(|&(root, _)| root == node);
        // A child holding every other child (a program node beside an end
        // token) wraps the statements.
        let mut top = tree.root;
        loop {
            let children: Vec<u32> = tree.children(top).iter().map(|edge| edge.node).filter(|&node| syntax(node)).collect();
            let holds_all = |child: u32| {
                let (start, end) = (tree.node(child).start, tree.node(child).end);
                children.iter().map(|&other| tree.node(other))
                    .all(|other| other.start == other.end || other.start >= start && other.end <= end)
            };
            match children.iter().copied().find(|&child| holds_all(child) && !tree.children(child).is_empty()) {
                Some(wrapper) => top = wrapper,
                None => break,
            }
        }
        let mut statements: Vec<Part> = Vec::new();
        let mut previous = 0u32;
        for node in tree.children(top).iter().map(|edge| edge.node).filter(|&node| syntax(node)) {
            let node = tree.node(node);
            if node.end <= previous { continue }
            statements.push(Part { start: previous, end: node.end, ..Part::default() });
            previous = node.end;
        }
        let ends: Vec<u32> = statements.iter().map(|part| part.end).collect();
        let statement_at = |at: u32| {
            let index = ends.partition_point(|&end| end <= at);
            (index < ends.len()).then_some(index)
        };
        let hash_span = |start: u32, end: u32| {
            use std::hash::{BuildHasher, Hasher};
            let mut hasher = crate::fx_hash::FxBuild::default().build_hasher();
            let mut at = start;
            for &(body_start, body_end) in body_spans.iter().filter(|&&(s, e)| s >= start && e <= end) {
                hasher.write(slice(at, body_start));
                hasher.write_u8(0xFF);
                at = body_end;
            }
            hasher.write(slice(at, end));
            hasher.finish()
        };
        let outside = hash_span(0, text.len() as u32);
        for part in &mut statements {
            part.hash = hash_span(part.start, part.end);
            part.shell = part.hash;
        }
        // The statement's text outside its members, bodies blanked as in
        // `hash_span`: equal to its hash without members.
        let shell_hash = |part: &Part| {
            use std::hash::{BuildHasher, Hasher};
            let inside_member = |start: u32, end: u32| part.members.iter().any(|member| start >= member.start && end <= member.end);
            let mut skips: Vec<(u32, u32, bool)> = part.members.iter().map(|member| (member.start, member.end, false)).collect();
            skips.extend(body_spans.iter().filter(|&&(start, end)| start >= part.start && end <= part.end && !inside_member(start, end)).map(|&(start, end)| (start, end, true)));
            skips.sort_unstable();
            let mut hasher = crate::fx_hash::FxBuild::default().build_hasher();
            let mut at = part.start;
            for (start, end, body) in skips {
                if start < at { continue }
                hasher.write(slice(at, start));
                if body { hasher.write_u8(0xFF) }
                at = end;
            }
            hasher.write(slice(at, part.end));
            hasher.finish()
        };
        let mut bodies: Vec<Part> = body_spans.iter().map(|&(start, end)| Part { start, end, hash: reads::hash_bytes(slice(start, end)), ..Part::default() }).collect();
        // What the file publishes is known from its tree alone.
        let mut published_by: Vec<Vec<Name>> = vec![Vec::new(); statements.len()];
        for (name, node) in published {
            let Some(statement) = statement_at(tree.node(*node).start) else { continue };
            match name {
                Some(name) => published_by[statement].push(name_hash(name)),
                None => statements[statement].computed = true,
            }
        }
        for (part, mut names) in statements.iter_mut().zip(published_by.iter().cloned()) {
            names.sort_unstable();
            names.dedup();
            part.declares = names.into_boxed_slice();
        }
        let mut declared_names = FxHashMap::default();
        let precise = names.is_some_and(|names| names.complete && names.failures.is_empty());
        let failed = names.is_some() && !precise;
        if names.is_none() {
            // Unresolved: every word may be a name declared or mentioned.
            let words = |start: u32, end: u32, skip: &[(u32, u32)]| -> Box<[Name]> {
                let mut found: Vec<Name> = Vec::new();
                let mut at = start;
                let mut spans = Vec::new();
                for &(s, e) in skip.iter().filter(|&&(s, e)| s >= start && e <= end) { spans.push((at, s)); at = e }
                spans.push((at, end));
                for (from, to) in spans {
                    let span = std::str::from_utf8(slice(from, to)).unwrap_or_default();
                    found.extend(span.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')).filter(|w| !w.is_empty()).map(name_hash));
                }
                found.sort_unstable();
                found.dedup();
                found.into_boxed_slice()
            };
            for part in &mut statements {
                part.mentions = words(part.start, part.end, &body_spans);
                let mut declares: Vec<Name> = part.declares.iter().chain(part.mentions.iter()).copied().collect();
                declares.sort_unstable();
                declares.dedup();
                part.declares = declares.into_boxed_slice();
            }
            for part in &mut bodies { part.mentions = words(part.start, part.end, &[]) }
        }
        if let (Some(names), true) = (names, precise) {
            let key = |node: u32| -> Name {
                match names.keys.get(&node) {
                    Some(Some(key)) => name_hash(key),
                    _ => {
                        let syntax = tree.node(node);
                        name_hash(std::str::from_utf8(slice(syntax.start, syntax.end)).unwrap_or_default())
                    }
                }
            };
            let module_scopes: Vec<u32> = names.scopes.iter().enumerate().filter(|(_, scope)| scope.node == tree.root).map(|(index, _)| index as u32).collect();
            let mut mentions: Vec<Vec<Name>> = vec![Vec::new(); statements.len()];
            let mut body_mentions: Vec<Vec<Name>> = vec![Vec::new(); bodies.len()];
            let mut declares: Vec<Vec<Name>> = published_by;
            let mut refers: Vec<Vec<u32>> = vec![Vec::new(); statements.len()];
            let mut body_refers: Vec<Vec<u32>> = vec![Vec::new(); bodies.len()];
            let mut outside_mentions: Vec<(u32, Name)> = Vec::new();
            let mut mention = |at: u32, name: Name| match body_at(at) {
                Some(body) => body_mentions[body].push(name),
                None => if let Some(statement) = statement_at(at) {
                    mentions[statement].push(name);
                    outside_mentions.push((at, name));
                },
            };
            // Only top-level names matter to other files: locals are reached
            // only through their statements.
            for (&node, &(scope, ..)) in names.declared.iter() {
                if !module_scopes.contains(&scope) { continue }
                let name = key(node);
                declared_names.insert(node, name);
                mention(tree.node(node).start, name);
                if let Some(statement) = statement_at(tree.node(node).start) { declares[statement].push(name) }
            }
            for (&used, &declaration) in names.declaration.iter() {
                let Some(&name) = declared_names.get(&declaration) else { continue };
                let at = tree.node(used).start;
                mention(at, name);
                let Some(target) = statement_at(tree.node(declaration).start) else { continue };
                match body_at(at) {
                    Some(body) => body_refers[body].push(target as u32),
                    None => if let Some(statement) = statement_at(at).filter(|&statement| statement != target) { refers[statement].push(target as u32) },
                }
            }
            let names_of = |mut list: Vec<Name>| { list.sort_unstable(); list.dedup(); list.into_boxed_slice() };
            let indices = |mut list: Vec<u32>| { list.sort_unstable(); list.dedup(); list.into_boxed_slice() };
            let mut members = members(tree, names, &module_scopes, &statements, &key, &syntax);
            for (at, name) in outside_mentions {
                let Some(statement) = statement_at(at) else { continue };
                let list = &mut members[statement];
                let after = list.partition_point(|(member, _)| member.start <= at);
                if after > 0 && at < list[after - 1].0.end { list[after - 1].1.push(name) }
            }
            for (index, part) in statements.iter_mut().enumerate() {
                part.mentions = names_of(std::mem::take(&mut mentions[index]));
                part.declares = names_of(std::mem::take(&mut declares[index]));
                part.refers = indices(std::mem::take(&mut refers[index]));
                part.members = std::mem::take(&mut members[index]).into_iter().map(|(mut member, mentions)| {
                    member.hash = hash_span(member.start, member.end);
                    member.mentions = names_of(mentions);
                    member
                }).collect();
                part.shell = shell_hash(part);
            }
            for (index, part) in bodies.iter_mut().enumerate() {
                part.mentions = names_of(std::mem::take(&mut body_mentions[index]));
                part.refers = indices(std::mem::take(&mut body_refers[index]));
            }
        }
        FileParts::new((unit.path.clone(), unit.hash), outside, statements, bodies, (precise, failed), declared_names, coarse)
    }
}

/// Per statement, its members (start, end and declared names; hashes come
/// later) with room for their mentions: the declaring nodes of the scopes
/// the statement opens below the module scope, outermost only (a method's
/// parameters belong to the method), each with the text since its previous
/// sibling. A declaration holding its own scope's opener (a class naming
/// itself inside) is the statement's, not a member.
fn members(tree: &dx_tree::Tree, names: &Resolved, module_scopes: &[u32], statements: &[Part], key: impl Fn(u32) -> Name, syntax: impl Fn(u32) -> bool)
    -> Vec<Vec<(Member, Vec<Name>)>> {
    let mut by_node: FxHashMap<u32, Vec<Name>> = FxHashMap::default();
    for (&node, &(scope, declaring, ..)) in names.declared.iter() {
        if module_scopes.contains(&scope) { continue }
        let Some(opener) = names.scopes.get(scope as usize).map(|info| tree.node(info.node)) else { continue };
        let span = tree.node(declaring);
        if span.start <= opener.start && opener.end <= span.end { continue }
        by_node.entry(declaring).or_default().push(key(node));
    }
    let ends: Vec<u32> = statements.iter().map(|part| part.end).collect();
    let mut found: Vec<Vec<(u32, Member)>> = vec![Vec::new(); statements.len()];
    for (node, mut declares) in by_node {
        let span = tree.node(node);
        let statement = ends.partition_point(|&end| end <= span.start);
        if statement >= statements.len() || span.end > statements[statement].end { continue }
        let before = tree.parent(node).and_then(|parent| {
            let siblings: Vec<u32> = tree.children(parent).iter().map(|edge| edge.node).filter(|&child| syntax(child)).collect();
            let at = siblings.iter().position(|&child| child == node)?;
            at.checked_sub(1).map(|previous| tree.node(siblings[previous]).end)
        });
        declares.sort_unstable();
        declares.dedup();
        let start = before.filter(|&end| end >= statements[statement].start && end <= span.start).unwrap_or(span.start);
        found[statement].push((span.start, Member { start, end: span.end, declares: declares.into(), ..Member::default() }));
    }
    found.into_iter().map(|mut list| {
        list.sort_unstable_by_key(|(at, member)| (*at, std::cmp::Reverse(member.end)));
        let mut kept: Vec<(Member, Vec<Name>)> = Vec::new();
        for (at, mut member) in list {
            if let Some((last, _)) = kept.last() {
                // Inside the previous one: a nested scope's, read through it.
                if at < last.end { continue }
                member.start = member.start.max(last.end);
            }
            kept.push((member, Vec::new()));
        }
        kept
    }).collect()
}
