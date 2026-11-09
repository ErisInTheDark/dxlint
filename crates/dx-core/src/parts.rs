//! What linting one file read of another: which of its top-level
//! statements, which names it looked up there, and which names' uses it
//! read. A file's edit then breaks only the readers whose parts it changed,
//! instead of everyone reaching the file.
//!
//! A [`FileParts`] describes one version of a file: its top-level
//! statements (each with its leading trivia, and with its opaque bodies
//! blanked: other files see them only by entering them), its bodies, the
//! names each declares and mentions, and which statements each one's names
//! resolve to. Reads are recorded per thread ([`note`]) against the file's
//! [`Owner`]; the engine turns them into deps. Comparing two versions
//! ([`FileParts::diff`]) is cheap: statements match by content, so
//! statements that moved are unchanged.

use std::cell::RefCell;
use std::hash::{BuildHasher, Hash};
use std::path::Path;
use std::sync::{Arc, OnceLock};

use crate::fx_hash::{FxBuild, FxHashSet};

/// A name as parts record it: a hash, so two names may share one (which
/// only ever makes a read look changed).
pub type Name = u32;

pub fn name_hash(key: &str) -> Name {
    let hash = FxBuild::default().hash_one(key);
    (hash ^ hash >> 32) as Name
}

/// One top-level statement, or one opaque body.
#[derive(Clone, Debug, Default)]
pub struct Part {
    pub start: u32,
    pub end: u32,
    /// Its text (opaque bodies blanked, for statements).
    pub hash: u64,
    /// Names it declares or publishes at the top level ([`name_hash`]).
    pub declares: Box<[Name]>,
    /// It publishes names nobody can list (a computed export).
    pub computed: bool,
    /// Top-level names it uses or declares, anywhere inside.
    pub mentions: Box<[Name]>,
    /// The statements its uses resolve to.
    pub refers: Box<[u32]>,
}

/// One version of a file, as its readers' deps see it.
#[derive(Debug)]
pub struct FileParts {
    /// The file (interned): reads of a version name it through it.
    pub path: crate::reads::DepPath,
    /// The whole text's hash: equal hashes, equal parts.
    pub hash: u64,
    /// The text with the opaque bodies blanked, hashed with their places.
    pub outside: u64,
    pub statements: Box<[Part]>,
    /// The opaque bodies, in the engine's body order.
    pub bodies: Box<[Part]>,
    /// Names resolved completely: else reads are judged by what the file
    /// publishes, or as reads of the whole file.
    pub precise: bool,
    /// Its names failed to resolve (a version described without resolving
    /// them, from its words alone, has not failed).
    pub failed: bool,
    /// Declaring name nodes -> their names, for reads of a declaration's uses.
    pub declared: crate::fx_hash::FxHashMap<u32, Name>,
    /// The file outside its bodies, as `.dxcache` keeps every read of it
    /// (`None`: a file without bodies, kept as its whole text).
    pub coarse: Option<Arc<crate::reads::Bodies>>,
    /// Per statement: the statements its reads may reach (itself included).
    reach: OnceLock<Box<[Box<[u64]>]>>,
}

// A version is its file and text: equal hashes, equal parts.
impl PartialEq for FileParts {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.path == other.path
    }
}
impl Eq for FileParts {}
impl Hash for FileParts {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state)
    }
}
impl PartialOrd for FileParts {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for FileParts {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (&self.path, self.hash).cmp(&(&other.path, other.hash))
    }
}

/// What a reader read of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Read {
    /// A top-level statement (by index), and what its names resolve to.
    Statement(u32),
    /// An opaque body (by index) it stepped into.
    Body(u32),
    /// A lookup of a top-level name, found or not.
    Name(Name),
    /// Every use of a name in the file (who reads or writes a binding).
    Uses(Name),
    /// Every use of the declaration at a name node (see [`FileParts::declared`]).
    UsesOf(u32),
    /// Everything outside the bodies it did not step into.
    Whole,
    /// The statements holding bytes `start..end`; recorded as those
    /// statements ([`end`]).
    Span(u32, u32),
}

/// Everything one piece of work read of one file, compactly.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartReads {
    pub statements: Box<[u32]>,
    pub bodies: Box<[u32]>,
    pub names: Box<[Name]>,
    pub uses: Box<[Name]>,
    pub uses_of: Box<[u32]>,
    pub whole: bool,
}

impl PartReads {
    pub fn new(reads: impl IntoIterator<Item = Read>) -> PartReads {
        let (mut statements, mut bodies, mut names, mut uses, mut uses_of, mut whole) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), false);
        for read in reads {
            match read {
                Read::Statement(index) => statements.push(index),
                Read::Body(index) => bodies.push(index),
                Read::Name(name) => names.push(name),
                Read::Uses(name) => uses.push(name),
                Read::UsesOf(node) => uses_of.push(node),
                Read::Whole | Read::Span(..) => whole = true,
            }
        }
        fn sorted<T: Ord>(mut list: Vec<T>) -> Box<[T]> { list.sort_unstable(); list.dedup(); list.into_boxed_slice() }
        PartReads { statements: sorted(statements), bodies: sorted(bodies), names: sorted(names), uses: sorted(uses), uses_of: sorted(uses_of), whole }
    }

    pub fn iter(&self) -> impl Iterator<Item = Read> + '_ {
        self.statements.iter().map(|&i| Read::Statement(i))
            .chain(self.bodies.iter().map(|&i| Read::Body(i)))
            .chain(self.names.iter().map(|&n| Read::Name(n)))
            .chain(self.uses.iter().map(|&n| Read::Uses(n)))
            .chain(self.uses_of.iter().map(|&n| Read::UsesOf(n)))
            .chain(self.whole.then_some(Read::Whole))
    }
}

/// How a version changed from an older one.
#[derive(Debug, Default)]
pub struct Diff {
    /// Everything may have changed (names failed or recovered).
    pub all: bool,
    /// Old statements gone or changed.
    removed: FxHashSet<u32>,
    /// Statements came, went or changed.
    pub statements: bool,
    /// Names declared by statements that came, went or changed (`any`: a
    /// computed publication among them).
    declared: FxHashSet<Name>,
    any_declared: bool,
    /// Names mentioned by statements or bodies that came, went or changed.
    mentioned: FxHashSet<Name>,
    /// Old bodies gone or changed.
    removed_bodies: FxHashSet<u32>,
    /// The text outside the bodies is the same, byte for byte and place for
    /// place; `changed_in_place` lists the bodies whose text changed then.
    same_outside: bool,
    changed_in_place: FxHashSet<u32>,
}

impl FileParts {
    pub fn new((path, hash): (crate::reads::DepPath, u64), outside: u64, statements: Vec<Part>, bodies: Vec<Part>, (precise, failed): (bool, bool), declared: crate::fx_hash::FxHashMap<u32, Name>,
        coarse: Option<Arc<crate::reads::Bodies>>) -> FileParts {
        FileParts { path, hash, outside, statements: statements.into(), bodies: bodies.into(), precise, failed, declared, coarse, reach: OnceLock::new() }
    }

    /// A copy made inside [`crate::alloc::lasting`]: versions live as long
    /// as reads of them, past the work that described them.
    pub fn lasting(self) -> FileParts {
        let _owner = crate::alloc::heap::Owner::enter(crate::alloc::heap::FILE_VERSIONS);
        crate::alloc::lasting(|| FileParts {
            statements: self.statements.to_vec().into(),
            bodies: self.bodies.to_vec().into(),
            declared: self.declared.clone(),
            coarse: self.coarse.clone(),
            reach: OnceLock::new(),
            ..self
        })
    }

    /// The statement holding byte `at`.
    pub fn statement_at(&self, at: u32) -> Option<u32> {
        let after = self.statements.partition_point(|part| part.start <= at);
        (after > 0 && at < self.statements[after - 1].end).then(|| after as u32 - 1)
    }

    /// Reads of the statements holding bytes `start..end`: the whole file
    /// when some of them lie outside every statement.
    pub fn statements_in(&self, start: u32, end: u32) -> Vec<Read> {
        match (self.statement_at(start), self.statement_at(end.saturating_sub(1).max(start))) {
            (Some(first), Some(last)) => (first..=last).map(Read::Statement).collect(),
            _ => vec![Read::Whole],
        }
    }

    /// How `new` (a later version of the file) differs from this one.
    pub fn diff(&self, new: &FileParts) -> Diff {
        let same_outside = self.outside == new.outside && self.bodies.len() == new.bodies.len();
        let changed_in_place = if same_outside {
            self.bodies.iter().zip(new.bodies.iter()).enumerate().filter(|(_, (a, b))| a.hash != b.hash).map(|(index, _)| index as u32).collect()
        } else { FxHashSet::default() };
        // Names that stopped resolving change every answer about them. (An
        // older version described without names is judged by what it
        // publishes alone: see `holds`.)
        if new.failed {
            return Diff { all: true, statements: true, same_outside, changed_in_place, ..Diff::default() };
        }
        let mut diff = Diff { same_outside, changed_in_place, ..Diff::default() };
        let counts = |parts: &[Part]| {
            let mut counts = crate::fx_hash::FxHashMap::<u64, usize>::default();
            for part in parts { *counts.entry(part.hash).or_default() += 1 }
            counts
        };
        let note = |diff: &mut Diff, part: &Part| {
            diff.declared.extend(part.declares.iter().copied());
            diff.any_declared |= part.computed;
            diff.mentioned.extend(part.mentions.iter().copied());
        };
        // Statements match by content: one gone or changed, or one new.
        let (old, now) = (counts(&self.statements), counts(&new.statements));
        let mut left = now.clone();
        for (index, part) in self.statements.iter().enumerate() {
            match left.get_mut(&part.hash) {
                Some(count) if *count > 0 => *count -= 1,
                _ => {
                    diff.removed.insert(index as u32);
                    note(&mut diff, part);
                }
            }
        }
        let mut left = old;
        for part in new.statements.iter() {
            match left.get_mut(&part.hash) {
                Some(count) if *count > 0 => *count -= 1,
                _ => {
                    diff.statements = true;
                    note(&mut diff, part);
                }
            }
        }
        diff.statements |= !diff.removed.is_empty();
        let bodies = counts(&new.bodies);
        let mut left = bodies;
        for (index, part) in self.bodies.iter().enumerate() {
            match left.get_mut(&part.hash) {
                Some(count) if *count > 0 => *count -= 1,
                _ => {
                    diff.removed_bodies.insert(index as u32);
                    diff.mentioned.extend(part.mentions.iter().copied());
                }
            }
        }
        let mut left = counts(&self.bodies);
        for part in new.bodies.iter() {
            match left.get_mut(&part.hash) {
                Some(count) if *count > 0 => *count -= 1,
                _ => diff.mentioned.extend(part.mentions.iter().copied()),
            }
        }
        diff
    }

    /// Whether `read` of this version reads the same in the version `diff`
    /// leads to. `strict`: positions must stay too (answers holding this
    /// version's node ids), so only edits inside bodies nobody stepped
    /// into hold.
    pub fn holds(&self, diff: &Diff, read: Read, strict: bool) -> bool {
        if strict {
            return diff.same_outside && match read {
                Read::Body(index) => !diff.changed_in_place.contains(&index),
                _ => true,
            };
        }
        if diff.all { return false }
        if !self.precise {
            // Described without names: only what the file publishes is known.
            return match read {
                Read::Name(name) => !diff.any_declared && !diff.declared.contains(&name)
                    && !self.statements.iter().enumerate().any(|(index, part)| part.declares.contains(&name) && diff.removed.contains(&(index as u32))),
                Read::Body(index) => !diff.statements && !diff.removed_bodies.contains(&index),
                _ => !diff.statements,
            };
        }
        match read {
            Read::Whole | Read::Span(..) => !diff.statements,
            Read::Body(index) => !diff.removed_bodies.contains(&index) && self.bodies.get(index as usize).is_some_and(|body| self.reach_holds(diff, body.refers.iter().copied())),
            Read::Statement(index) => self.reach_holds(diff, std::iter::once(index)),
            Read::Name(name) => !diff.any_declared && !diff.declared.contains(&name)
                && self.reach_holds(diff, self.statements.iter().enumerate().filter(|(_, part)| part.declares.contains(&name)).map(|(index, _)| index as u32)),
            Read::Uses(name) => !diff.mentioned.contains(&name),
            Read::UsesOf(node) => self.declared.get(&node).is_some_and(|name| !diff.mentioned.contains(name)),
        }
    }

    /// Whether every read of `reads` reads the same in the version `diff`
    /// leads to ([`FileParts::holds`], with one reach for all of them).
    pub fn holds_all(&self, diff: &Diff, reads: &PartReads, strict: bool) -> bool {
        if strict || diff.all || !self.precise || reads.whole {
            return reads.iter().all(|read| self.holds(diff, read, strict));
        }
        if reads.bodies.iter().any(|index| diff.removed_bodies.contains(index)) { return false }
        if reads.uses.iter().any(|name| diff.mentioned.contains(name)) { return false }
        if reads.uses_of.iter().any(|node| self.declared.get(node).is_none_or(|name| diff.mentioned.contains(name))) { return false }
        if !reads.names.is_empty() && diff.any_declared { return false }
        if reads.names.iter().any(|name| diff.declared.contains(name)) { return false }
        let declaring = self.statements.iter().enumerate()
            .filter(|(_, part)| part.declares.iter().any(|name| reads.names.binary_search(name).is_ok()))
            .map(|(index, _)| index as u32);
        let from_bodies = reads.bodies.iter().filter_map(|&index| self.bodies.get(index as usize)).flat_map(|body| body.refers.iter().copied());
        self.reach_holds(diff, reads.statements.iter().copied().chain(declaring).chain(from_bodies))
    }

    /// Whether the statements reachable from `from` are unchanged, and no
    /// changed statement declares a name they use (it could shadow or join
    /// what they resolved to).
    fn reach_holds(&self, diff: &Diff, from: impl Iterator<Item = u32>) -> bool {
        if diff.any_declared { return !diff.statements }
        let reach = self.reach();
        let words = self.statements.len().div_ceil(64);
        let mut seen = vec![0u64; words];
        for index in from {
            let Some(bits) = reach.get(index as usize) else { return false };
            for (into, &bit) in seen.iter_mut().zip(bits.iter()) { *into |= bit }
        }
        for (index, part) in self.statements.iter().enumerate() {
            if seen[index / 64] >> (index % 64) & 1 == 0 { continue }
            if diff.removed.contains(&(index as u32)) { return false }
            if part.mentions.iter().any(|name| diff.declared.contains(name)) { return false }
        }
        true
    }

    fn reach(&self) -> &[Box<[u64]>] {
        self.reach.get_or_init(|| {
            let count = self.statements.len();
            let words = count.div_ceil(64);
            (0..count).map(|start| {
                let mut bits = vec![0u64; words].into_boxed_slice();
                let mut stack = vec![start as u32];
                while let Some(index) = stack.pop() {
                    let index = index as usize;
                    if index >= count || bits[index / 64] >> (index % 64) & 1 != 0 { continue }
                    bits[index / 64] |= 1 << (index % 64);
                    stack.extend(self.statements[index].refers.iter().copied());
                }
                bits
            }).collect()
        })
    }
}

/// A file as reads of it are recorded: its path and, once known, its parts.
#[derive(Debug)]
pub struct Owner {
    pub path: crate::reads::DepPath,
    /// The text's hash: reads of a file whose parts are not known yet
    /// depend on its whole text.
    pub hash: u64,
    /// Its parts: first without resolved names (statements and what the
    /// file publishes), then with them once they are known.
    parts: std::sync::Mutex<Option<Arc<FileParts>>>,
    /// The parts kept know the names: nothing later replaces them.
    settled: std::sync::atomic::AtomicBool,
}

impl Owner {
    pub fn new(path: crate::reads::DepPath, hash: u64) -> Owner {
        Owner { path, hash, parts: std::sync::Mutex::new(None), settled: std::sync::atomic::AtomicBool::new(false) }
    }

    pub fn parts(&self) -> Option<Arc<FileParts>> {
        self.parts.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn settled(&self) -> bool {
        self.settled.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Keeps `parts` unless the kept ones know at least as much.
    pub fn offer(&self, parts: FileParts) -> Arc<FileParts> {
        let mut kept = self.parts.lock().unwrap_or_else(|e| e.into_inner());
        match &*kept {
            Some(current) if current.precise || !parts.precise => current.clone(),
            _ => {
                let _owner = crate::alloc::heap::Owner::enter(crate::alloc::heap::FILE_VERSIONS);
                let parts = crate::alloc::lasting(|| Arc::new(parts));
                self.settled.store(parts.precise, std::sync::atomic::Ordering::Release);
                *kept = Some(parts.clone());
                parts
            }
        }
    }
}

thread_local! {
    /// Per recording frame: the reads noted, with their owners kept.
    static SINKS: RefCell<Vec<(FxHashSet<(*const Owner, Read)>, Vec<Arc<Owner>>)>> = const { RefCell::new(Vec::new()) };
    /// The file this thread lints: its own reads are not parts of another file.
    static LINTED: RefCell<Option<std::path::PathBuf>> = const { RefCell::new(None) };
}

/// Runs `f` linting `path` on this thread: reads of `path`'s own unit
/// record nothing here (the linted file's reads are its own business).
pub fn linting<R>(path: &Path, f: impl FnOnce() -> R) -> R {
    let outer = LINTED.with(|linted| linted.replace(Some(path.to_path_buf())));
    let answer = f();
    LINTED.with(|linted| linted.replace(outer));
    answer
}

thread_local! {
    static QUIET: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Runs `f` recording no part reads: a scan over many files, which records
/// what it asked each one its own way.
pub fn quiet<R>(f: impl FnOnce() -> R) -> R {
    struct Loud;
    impl Drop for Loud {
        fn drop(&mut self) { QUIET.with(|quiet| quiet.set(quiet.get() - 1)) }
    }
    QUIET.with(|quiet| quiet.set(quiet.get() + 1));
    let _loud = Loud;
    f()
}

/// Whether part reads go unrecorded here ([`quiet`]).
pub fn is_quiet() -> bool {
    QUIET.with(std::cell::Cell::get) > 0
}

/// Records `read` of `owner`'s file for the innermost frame ([`begin`]).
#[inline]
pub fn note(owner: &Arc<Owner>, read: Read) {
    if QUIET.with(std::cell::Cell::get) > 0 { return }
    SINKS.with(|sinks| {
        if let Some((seen, owners)) = sinks.borrow_mut().last_mut() {
            if seen.insert((Arc::as_ptr(owner), read)) && !owners.iter().any(|kept| Arc::ptr_eq(kept, owner)) {
                owners.push(owner.clone());
            }
        }
    });
}

/// Starts a recording frame; [`end`] it.
pub fn begin() {
    SINKS.with(|sinks| sinks.borrow_mut().push(Default::default()));
}

/// Ends the innermost frame, returning its reads, by file.
pub fn end() -> Vec<(Arc<Owner>, PartReads)> {
    let Some((seen, owners)) = SINKS.with(|sinks| sinks.borrow_mut().pop()) else { return Vec::new() };
    let own = |owner: &Owner| LINTED.with(|linted| linted.borrow().as_deref() == Some(&*owner.path));
    owners.into_iter().filter(|owner| !own(owner)).map(|owner| {
        let parts = owner.parts();
        let reads = seen.iter().filter(|(of, _)| *of == Arc::as_ptr(&owner)).flat_map(|&(_, read)| match (read, &parts) {
            (Read::Span(start, end), Some(parts)) => parts.statements_in(start, end),
            (read, _) => vec![read],
        });
        let reads = PartReads::new(reads);
        (owner, reads)
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(hash: u64, declares: &[&str], mentions: &[&str], refers: &[u32]) -> Part {
        Part {
            hash,
            declares: declares.iter().map(|n| name_hash(n)).collect(),
            mentions: mentions.iter().chain(declares).map(|n| name_hash(n)).collect(),
            refers: refers.into(),
            ..Part::default()
        }
    }

    fn file(hash: u64, statements: Vec<Part>) -> FileParts {
        FileParts::new((crate::reads::dep_path(Path::new("/parts/file.ts")), hash), hash, statements, Vec::new(), (true, false), Default::default(), None)
    }

    #[test]
    fn readers_hold_while_the_parts_they_read_stay() {
        // `type A = B`, `type B = number`, `const c = 1`.
        let old = file(1, vec![part(10, &["A"], &["B"], &[1]), part(20, &["B"], &[], &[]), part(30, &["c"], &[], &[])]);
        // An unrelated new export, appended.
        let added = file(2, vec![part(10, &["A"], &["B"], &[1]), part(20, &["B"], &[], &[]), part(30, &["c"], &[], &[]), part(40, &["d"], &[], &[])]);
        let diff = old.diff(&added);
        assert!(old.holds(&diff, Read::Name(name_hash("A")), false));
        assert!(old.holds(&diff, Read::Statement(0), false));
        assert!(!old.holds(&diff, Read::Name(name_hash("d")), false), "a lookup of the new name finds it now");
        assert!(!old.holds(&diff, Read::Whole, false) && !old.holds(&diff, Read::Statement(0), true));
        // `B` changes: `A` reaches it.
        let changed = file(3, vec![part(10, &["A"], &["B"], &[1]), part(21, &["B"], &[], &[]), part(30, &["c"], &[], &[])]);
        let diff = old.diff(&changed);
        assert!(!old.holds(&diff, Read::Name(name_hash("A")), false));
        assert!(old.holds(&diff, Read::Name(name_hash("c")), false));
        // A new statement declaring `B` again could join or shadow it.
        let merged = file(4, vec![part(10, &["A"], &["B"], &[1]), part(20, &["B"], &[], &[]), part(30, &["c"], &[], &[]), part(50, &["B"], &[], &[])]);
        assert!(!old.holds(&old.diff(&merged), Read::Statement(0), false));
        // A new statement using `c` breaks readers of `c`'s uses only.
        let uses = file(5, vec![part(10, &["A"], &["B"], &[1]), part(20, &["B"], &[], &[]), part(30, &["c"], &[], &[]), part(60, &[], &["c"], &[2])]);
        let diff = old.diff(&uses);
        assert!(!old.holds(&diff, Read::Uses(name_hash("c")), false));
        assert!(old.holds(&diff, Read::Uses(name_hash("A")), false) && old.holds(&diff, Read::Name(name_hash("c")), false));
    }

    #[test]
    fn statements_that_only_moved_are_unchanged() {
        let old = file(1, vec![part(10, &["a"], &[], &[]), part(20, &["b"], &[], &[])]);
        let moved = file(2, vec![part(20, &["b"], &[], &[]), part(10, &["a"], &[], &[])]);
        let diff = old.diff(&moved);
        assert!(!diff.statements && old.holds(&diff, Read::Whole, false));
    }

    #[test]
    fn spans_read_the_statements_holding_them_or_the_whole_file() {
        let at = |start, end| Part { start, end, ..part(start.into(), &[], &[], &[]) };
        let parts = file(1, vec![at(0, 10), at(10, 20), at(20, 30)]);
        assert_eq!(parts.statements_in(12, 25), [Read::Statement(1), Read::Statement(2)]);
        assert_eq!(parts.statements_in(10, 20), [Read::Statement(1)]);
        // Text past the last statement belongs to none of them.
        assert_eq!(parts.statements_in(25, 40), [Read::Whole]);
    }
}
