//! Names a language declares with `scope`, `declares` and `resolves` items
//! (docs/design/type-layers.md): which nodes open scopes, which declare
//! names into which enclosing scope, and which use a name. [`Names`] holds
//! those tables per matcher; [`Names::resolve`] answers them for one tree.
//!
//! Classes are bit positions: declaration classes (`value`, `type`) are
//! namespaces, scope classes (`function`, `block`) say where a declaration
//! lands. Both come from the items that name them.

use crate::config::ast::{Config, Name, NameItem};
use crate::config::error::ConfigError;
use crate::fx_hash::FxHashMap;

use super::{Grammar, Tree};

/// One `declares` item.
#[derive(Clone, Debug)]
pub struct Declare {
    /// Declaration classes (bits).
    pub classes: u64,
    /// Labels from the declaring node to the declared names.
    pub path: Vec<u16>,
    /// Scope classes (bits) of `in nearest`.
    pub nearest: u64,
    /// Visible in the whole scope, not only after its position.
    pub hoisted: bool,
}

/// One `resolves` item.
#[derive(Clone, Debug)]
pub struct Resolve {
    /// Declaration classes (bits) looked up.
    pub classes: u64,
    /// Labels from the node to the using name; empty: the node itself.
    pub path: Vec<u16>,
}

/// What one matcher's nodes do with names.
#[derive(Clone, Debug, Default)]
pub struct KindNames {
    /// Scope classes (bits) the node opens; 0 when it opens none.
    pub scope: u64,
    pub declares: Vec<Declare>,
    pub resolves: Vec<Resolve>,
    /// `declares through`: when something declares this node, it declares
    /// what these lead to instead; per group, the first path that leads
    /// anywhere.
    pub through: Vec<Vec<Vec<u16>>>,
}

/// The tables, per matcher (empty when the config has no name items).
#[derive(Default)]
pub struct Names {
    pub declaration_classes: Vec<String>,
    pub scope_classes: Vec<String>,
    pub kinds: Vec<KindNames>,
}

/// One tree's resolved names.
#[derive(Default)]
pub struct Resolved {
    /// Using name -> the name node that declares it (the first of merged
    /// declarations).
    pub declaration: FxHashMap<u32, u32>,
    /// Declaring name (the first of merged ones) -> using names, in order.
    pub references: FxHashMap<u32, Vec<u32>>,
}

/// A declaration in one scope: name node, start, classes, hoisted.
type Declared = (u32, u32, u64, bool);

struct Scope {
    classes: u64,
    parent: Option<usize>,
    /// Name -> its declarations in document order.
    declared: FxHashMap<Box<str>, Vec<Declared>>,
}

impl Names {
    /// The tables of `config`'s name items. Classes come from the items
    /// naming them; one looked up or searched for but never declared or
    /// opened is an error, like an unknown matcher.
    pub fn build(config: &Config, g: &Grammar, errors: &mut Vec<ConfigError>) -> Names {
        let mut names = Names { kinds: vec![KindNames::default(); g.matchers.len()], ..Names::default() };
        if config.names.is_empty() {
            return names;
        }
        let mut declared = 0u64;
        let mut opened = 0u64;
        let mut asked: Vec<(&Name, bool)> = Vec::new();
        for def in &config.names {
            let targets: Vec<u32> = def
                .targets
                .iter()
                .filter_map(|t| match g.matcher(&t.text) {
                    Some(m) => Some(m),
                    None => {
                        errors.push(ConfigError::new(t.span, format!("unknown matcher `{}`", t.text)).suggest(&t.text, g.matchers.iter().map(|m| m.name.as_str())));
                        None
                    }
                })
                .collect();
            let path = |errors: &mut Vec<ConfigError>, path: &[Name]| -> Option<Vec<u16>> {
                path.iter()
                    .map(|f| {
                        g.labels.get(&f.text).ok_or_else(|| errors.push(ConfigError::new(f.span, format!("no layer has a field `{}`", f.text))))
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .ok()
            };
            match &def.item {
                NameItem::Scope(classes) => {
                    let Some(bits) = class_bits(&mut names.scope_classes, classes, errors) else { continue };
                    opened |= bits;
                    for &m in &targets {
                        names.add(g, m, |k| k.scope |= bits);
                    }
                }
                NameItem::Declares { classes, path: p, nearest, hoisted } => {
                    let Some(bits) = class_bits(&mut names.declaration_classes, classes, errors) else { continue };
                    let Some(near) = class_bits(&mut names.scope_classes, nearest, errors) else { continue };
                    declared |= bits;
                    asked.extend(nearest.iter().map(|n| (n, true)));
                    let Some(p) = path(errors, p) else { continue };
                    let d = Declare { classes: bits, path: p, nearest: near, hoisted: *hoisted };
                    for &m in &targets {
                        names.add(g, m, |k| k.declares.push(d.clone()));
                    }
                }
                NameItem::Through(groups) => {
                    let mut through = Vec::new();
                    for group in groups {
                        let paths: Option<Vec<Vec<u16>>> = group.iter().map(|p| path(errors, p)).collect();
                        let Some(paths) = paths else { continue };
                        through.push(paths);
                    }
                    for &m in &targets {
                        names.add(g, m, |k| k.through.extend(through.iter().cloned()));
                    }
                }
                NameItem::Resolves { classes, path: p } => {
                    let Some(bits) = class_bits(&mut names.declaration_classes, classes, errors) else { continue };
                    asked.extend(classes.iter().map(|c| (c, false)));
                    let Some(p) = path(errors, p) else { continue };
                    let r = Resolve { classes: bits, path: p };
                    for &m in &targets {
                        names.add(g, m, |k| k.resolves.push(r.clone()));
                    }
                }
            }
        }
        for (name, scope) in asked {
            let (list, have, what) = if scope {
                (&names.scope_classes, opened, "no layer opens a scope of class")
            } else {
                (&names.declaration_classes, declared, "nothing declares class")
            };
            let bit = list.iter().position(|c| *c == name.text).map_or(0, |i| 1u64 << i);
            if have & bit == 0 {
                errors.push(ConfigError::new(name.span, format!("{what} `{}`", name.text)).suggest(&name.text, list.iter().map(String::as_str)));
            }
        }
        names
    }

    /// Applies `f` to `m` and every matcher specialising it (`like`).
    fn add(&mut self, g: &Grammar, m: u32, f: impl Fn(&mut KindNames)) {
        for (i, kind) in self.kinds.iter_mut().enumerate() {
            if g.is_a(i as u32, m) {
                f(kind);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.kinds.iter().all(|k| k.scope == 0 && k.declares.is_empty() && k.resolves.is_empty())
    }

    /// The names a declaration of `node` declares: the node itself, or
    /// what its `declares through` paths lead to, recursively.
    fn bindings(&self, tree: &Tree, node: u32, out: &mut Vec<u32>, depth: u32) {
        let through = self.kinds.get(tree.node(node).matcher as usize).map_or(&[][..], |k| &k.through[..]);
        if through.is_empty() || depth > 256 {
            out.push(node);
            return;
        }
        // Per group, the first alternative that declares anything:
        // `value ?? key` falls back to the key when the value is a pattern
        // declaring nothing (`{ a = 1 }`).
        for group in through {
            for path in group {
                let before = out.len();
                for n in follow(tree, node, path) {
                    self.bindings(tree, n, out, depth + 1);
                }
                if out.len() > before {
                    break;
                }
            }
        }
    }

    /// Resolves every name `tree` uses.
    pub fn resolve(&self, tree: &Tree, text: &str) -> Resolved {
        let mut out = Resolved::default();
        if self.is_empty() {
            return out;
        }
        let name_text = |id: u32| {
            let n = tree.node(id);
            &text[n.start as usize..n.end as usize]
        };
        let mut scopes: Vec<Scope> = Vec::new();
        // Node -> the innermost scope it sits in (its own, when it opens one).
        let mut scope_of: FxHashMap<u32, usize> = FxHashMap::default();
        let mut uses: Vec<(u32, usize, u64)> = Vec::new();
        for &id in &tree.order {
            let outer = tree.parent(id).and_then(|p| scope_of.get(&p).copied());
            let kind = self.kinds.get(tree.node(id).matcher as usize);
            let here = match kind.filter(|k| k.scope != 0) {
                Some(k) => {
                    scopes.push(Scope { classes: k.scope, parent: outer, declared: FxHashMap::default() });
                    Some(scopes.len() - 1)
                }
                None => outer,
            };
            if let Some(here) = here {
                scope_of.insert(id, here);
            }
            let Some(kind) = kind else { continue };
            for d in &kind.declares {
                let mut target = here;
                while let Some(s) = target.filter(|&s| scopes[s].classes & d.nearest == 0) {
                    target = scopes[s].parent;
                }
                let Some(target) = target else { continue };
                let mut declared = Vec::new();
                for named in follow(tree, id, &d.path) {
                    self.bindings(tree, named, &mut declared, 0);
                }
                for name in declared {
                    let start = tree.node(name).start;
                    scopes[target].declared.entry(name_text(name).into()).or_default().push((name, start, d.classes, d.hoisted));
                }
            }
            if let Some(here) = here {
                for r in &kind.resolves {
                    for name in follow(tree, id, &r.path) {
                        uses.push((name, here, r.classes));
                    }
                }
            }
        }
        for (name, scope, want) in uses {
            let start = tree.node(name).start;
            let key = name_text(name);
            let mut at = Some(scope);
            while let Some(s) = at {
                let decls = scopes[s].declared.get(key);
                if let Some(found) = decls.and_then(|decls| visible(decls, start, want)) {
                    let found = decls.map_or(found, |decls| merged(decls, found));
                    if found != name {
                        out.declaration.insert(name, found);
                        out.references.entry(found).or_default().push(name);
                    }
                    break;
                }
                at = scopes[s].parent;
            }
        }
        for refs in out.references.values_mut() {
            refs.sort_by_key(|&r| tree.node(r).start);
        }
        out
    }
}

/// The nodes `path` (field labels) leads to from `from`.
fn follow(tree: &Tree, from: u32, path: &[u16]) -> Vec<u32> {
    let mut nodes = vec![from];
    for &label in path {
        nodes = nodes.into_iter().flat_map(|n| tree.fields(n, label)).collect();
    }
    nodes
}

/// The bits of `classes`, adding new ones to `list` (at most 64).
fn class_bits(list: &mut Vec<String>, classes: &[Name], errors: &mut Vec<ConfigError>) -> Option<u64> {
    let mut bits = 0;
    for c in classes {
        let i = match list.iter().position(|x| *x == c.text) {
            Some(i) => i,
            None if list.len() < 64 => {
                list.push(c.text.clone());
                list.len() - 1
            }
            None => {
                errors.push(ConfigError::new(c.span, "at most 64 classes of one kind"));
                return None;
            }
        };
        bits |= 1 << i;
    }
    Some(bits)
}

/// The declaration a use at `start` sees among one scope's declarations of
/// its name: the latest one before it that is not hoisted (shadowing), else
/// the first hoisted one (merged declarations share it).
fn visible(decls: &[Declared], start: u32, want: u64) -> Option<u32> {
    let matching = || decls.iter().filter(|d| d.2 & want != 0);
    matching().rfind(|d| !d.3 && d.1 <= start).or_else(|| matching().find(|d| d.3)).map(|d| d.0)
}

/// Hoisted declarations of one name in one scope are one binding, whatever
/// their classes (`interface X` + `namespace X`, overloads): the first.
fn merged(decls: &[Declared], found: u32) -> u32 {
    let hoisted = |d: &&Declared| d.3;
    match decls.iter().find(|d| d.0 == found) {
        Some(d) if d.3 => decls.iter().find(hoisted).map_or(found, |first| first.0),
        _ => found,
    }
}

#[cfg(test)]
mod tests {
    use super::{merged, visible};

    #[test]
    fn hoisted_declarations_of_any_class_merge_into_the_first() {
        const VALUE: u64 = 1;
        const TYPE: u64 = 2;
        // `type X` then `namespace X`: a value lookup finds the namespace,
        // which merges into the type alias.
        let decls = [(1, 10, TYPE, true), (2, 20, VALUE | TYPE, true), (3, 30, VALUE, false)];
        assert_eq!(merged(&decls, visible(&decls, 40, VALUE).expect("visible")), 3);
        assert_eq!(merged(&decls, 2), 1);
        assert_eq!(merged(&decls, 3), 3);
    }

    #[test]
    fn later_declarations_shadow_and_hoisted_ones_merge_into_the_first() {
        const VALUE: u64 = 1;
        const TYPE: u64 = 2;
        // `let x` at 10 and 30: a use at 20 sees the first, at 40 the second.
        let lets = [(1, 10, VALUE, false), (2, 30, VALUE, false)];
        assert_eq!(visible(&lets, 5, VALUE), None);
        assert_eq!(visible(&lets, 20, VALUE), Some(1));
        assert_eq!(visible(&lets, 40, VALUE), Some(2));
        // `var x` twice: one binding, the first, visible before both.
        let vars = [(3, 10, VALUE, true), (4, 30, VALUE, true)];
        assert_eq!(visible(&vars, 5, VALUE), Some(3));
        assert_eq!(visible(&vars, 40, VALUE), Some(3));
        // Namespaces: a type declaration is invisible to value lookups.
        let both = [(5, 10, TYPE, true), (6, 20, VALUE, true)];
        assert_eq!(visible(&both, 30, VALUE), Some(6));
        assert_eq!(visible(&both, 30, TYPE), Some(5));
    }
}
