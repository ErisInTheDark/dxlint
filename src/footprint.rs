//! What a computation read of the file an editor lints, so the editor's
//! next lint of that file keeps every answer an edit leaves standing.
//!
//! A footprint is the set of atoms read (parts of the file's chunks, and
//! tables derived from it), each kept as a 32-bit fingerprint, plus the
//! byte range of the file whose positions were read (offsets, lines,
//! columns: their differences change only when an edit lands inside that
//! range). An edit's [`Dirty`] says which atoms it changed; a footprint
//! that [`Footprint::meets`] it may read differently now.
//!
//! Reads are recorded per thread into the computation in progress
//! ([`begin`]/[`end`], a [`Reading`]); memoised answers keep their
//! footprint and hand it to every later reader ([`enter`]).

use std::cell::Cell;

/// What a finished computation read: kept with its answer.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Footprint {
    /// Atom fingerprints, ascending.
    atoms: Vec<u32>,
    /// Read everything.
    all: bool,
    /// Byte range whose offsets or text were read; empty when `from > to`.
    from: u32,
    to: u32,
    /// Byte range whose line numbers were read: they change only with the
    /// number of lines.
    lines_from: u32,
    lines_to: u32,
}

impl Default for Footprint {
    fn default() -> Self {
        Self::NONE
    }
}

impl Footprint {
    pub const NONE: Footprint = Footprint { atoms: Vec::new(), all: false, from: u32::MAX, to: 0, lines_from: u32::MAX, lines_to: 0 };

    /// Reads everything: changed by every edit.
    pub fn all() -> Footprint {
        Footprint { all: true, from: 0, to: u32::MAX, lines_from: 0, lines_to: u32::MAX, ..Self::NONE }
    }

    pub fn atom(atom: Atom) -> Footprint {
        Footprint { atoms: vec![atom.key()], ..Self::NONE }
    }

    /// Read the positions of bytes `from..=to`.
    pub fn positions(from: u32, to: u32) -> Footprint {
        Footprint { from: from.min(to), to: from.max(to), ..Self::NONE }
    }

    /// Read the line number of byte `at`.
    pub fn line(at: u32) -> Footprint {
        Footprint { lines_from: at, lines_to: at, ..Self::NONE }
    }

    pub fn with(&self, other: &Footprint) -> Footprint {
        let mut reading = Reading::NONE;
        reading.add(self);
        reading.add(other);
        reading.footprint()
    }

    /// The byte range whose offsets or text were read, if any.
    pub fn range(&self) -> Option<(u32, u32)> {
        (self.from <= self.to).then_some((self.from, self.to))
    }

    /// Atoms read (`u32::MAX`: everything).
    pub fn count(&self) -> u32 {
        if self.all { u32::MAX } else { self.atoms.len() as u32 }
    }

    /// Whether `dirty` meets this through its atoms (not positions).
    pub fn meets_atoms(&self, dirty: &Dirty) -> bool {
        if self.all || dirty.all { return (self.all || !self.atoms.is_empty()) && (dirty.all || !dirty.atoms.is_empty()) }
        if dirty.atoms.is_empty() { return false }
        self.atoms.iter().any(|&atom| dirty.filter[(atom as usize / 64) % FILTER] >> (atom % 64) & 1 != 0 && dirty.atoms.binary_search(&atom).is_ok())
    }

    pub fn is_none(&self) -> bool {
        !self.all && self.atoms.is_empty() && self.from > self.to && self.lines_from > self.lines_to
    }

    /// Whether what this read may read differently after `dirty`'s edit.
    pub fn meets(&self, dirty: &Dirty) -> bool {
        self.meets_atoms(dirty)
            || dirty.edit.is_some_and(|edit| edit.touches(self.from, self.to)
                || edit.lines && edit.touches(self.lines_from, self.lines_to))
    }

    /// Moves these reads to the edited text's positions (for a footprint
    /// the edit does not meet: all its positions lie before or after the
    /// edit, or only its line count changed none of them).
    pub fn shift(&mut self, dirty: &Dirty) {
        if let Some(edit) = dirty.edit {
            let shift = |from: &mut u32, to: &mut u32| if *from <= *to && *from > edit.old_end {
                *from = edit.moved(*from);
                *to = edit.moved(*to);
            };
            shift(&mut self.from, &mut self.to);
            shift(&mut self.lines_from, &mut self.lines_to);
        }
    }

    /// These reads in the edited text's positions (see [`Footprint::shift`]).
    pub fn shifted(mut self, dirty: &Dirty) -> Footprint {
        self.shift(dirty);
        self
    }
}

/// What the computation in progress read so far: its atoms in reading
/// order (repeats mostly skipped, see [`SEEN`]).
#[derive(Debug, Default)]
pub struct Reading {
    atoms: Vec<u32>,
    all: bool,
    from: u32,
    to: u32,
    lines_from: u32,
    lines_to: u32,
    /// Names this reading in [`SEEN`] (0: not yet).
    generation: u32,
    /// `atoms`' length that makes it drop its repeats.
    limit: u32,
}

impl Reading {
    pub const NONE: Reading = Reading { atoms: Vec::new(), all: false, from: u32::MAX, to: 0, lines_from: u32::MAX, lines_to: 0, generation: 0, limit: 0 };
    /// Atoms gathered before repeats are dropped the first time.
    const LIMIT: u32 = 4096;

    fn add(&mut self, footprint: &Footprint) {
        self.extend(&footprint.atoms);
        self.all |= footprint.all;
        self.from = self.from.min(footprint.from);
        self.to = self.to.max(footprint.to);
        self.lines_from = self.lines_from.min(footprint.lines_from);
        self.lines_to = self.lines_to.max(footprint.lines_to);
    }

    /// Takes in what `other` read.
    fn absorb(&mut self, other: Reading) {
        self.extend(&other.atoms);
        self.all |= other.all;
        self.from = self.from.min(other.from);
        self.to = self.to.max(other.to);
        self.lines_from = self.lines_from.min(other.lines_from);
        self.lines_to = self.lines_to.max(other.lines_to);
    }

    fn extend(&mut self, atoms: &[u32]) {
        if atoms.is_empty() { return }
        self.atoms.extend_from_slice(atoms);
        if self.atoms.len() as u32 > self.limit.max(Self::LIMIT) {
            self.atoms.sort_unstable();
            self.atoms.dedup();
            self.limit = self.atoms.len() as u32 * 2;
        }
    }

    /// The same reads, plus `atom`.
    pub fn with_atom(mut self, atom: Atom) -> Reading {
        self.extend(&[atom.key()]);
        self
    }

    /// The same reads, kept.
    fn footprint(mut self) -> Footprint {
        self.atoms.sort_unstable();
        self.atoms.dedup();
        self.atoms.shrink_to_fit();
        Footprint { atoms: self.atoms, all: self.all, from: self.from, to: self.to, lines_from: self.lines_from, lines_to: self.lines_to }
    }
}

/// One text edit: bytes `start..old_end` of the old text became
/// `start..new_end` of the new one; `lines` when it changed how many lines
/// there are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Edit {
    pub start: u32,
    pub old_end: u32,
    pub new_end: u32,
    pub lines: bool,
}

impl Edit {
    /// Where a position of the old text outside the edit is in the new text.
    pub fn moved(&self, at: u32) -> u32 {
        if at <= self.start { at } else { (i64::from(at) - i64::from(self.old_end) + i64::from(self.new_end)) as u32 }
    }

    /// Whether the edit lies in or against bytes `from..=to` (empty when
    /// `from > to`).
    fn touches(&self, from: u32, to: u32) -> bool {
        from <= self.old_end && to >= self.start && from <= to
    }
}

/// What an edit changed: atoms, and the edited range (positions read
/// across it read differently now).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Dirty {
    /// Atom fingerprints, ascending.
    atoms: Vec<u32>,
    /// A bloom of `atoms`, to rule most atoms out without a search.
    filter: [u64; FILTER],
    /// Every atom changed.
    all: bool,
    pub edit: Option<Edit>,
}

impl Dirty {
    pub fn edit(edit: Edit) -> Dirty {
        Dirty { edit: Some(edit), ..Dirty::default() }
    }

    /// Everything changed.
    pub fn all() -> Dirty {
        Dirty { all: true, ..Dirty::default() }
    }

    pub fn add(&mut self, footprint: &Footprint) {
        self.all |= footprint.all;
        self.insert(&footprint.atoms);
    }

    pub fn add_atom(&mut self, atom: Atom) {
        self.insert(&[atom.key()]);
    }

    fn insert(&mut self, atoms: &[u32]) {
        for &atom in atoms {
            if let Err(at) = self.atoms.binary_search(&atom) {
                self.atoms.insert(at, atom);
                self.filter[(atom as usize / 64) % FILTER] |= 1 << (atom % 64);
            }
        }
    }

    /// Whether this changed any atom (beyond the edited range).
    pub fn has_atoms(&self) -> bool {
        self.all || !self.atoms.is_empty()
    }

    /// The same atoms, without the edited range.
    pub fn atoms(&self) -> Dirty {
        Dirty { edit: None, ..self.clone() }
    }

    pub fn union(&mut self, other: &Dirty) {
        self.all |= other.all;
        self.insert(&other.atoms);
    }

    /// The atoms of this that `other` lacks (no edit).
    pub fn minus(&self, other: &Dirty) -> Dirty {
        if other.all { return Dirty::default() }
        let mut rest = Dirty { all: self.all, ..Dirty::default() };
        let atoms: Vec<u32> = self.atoms.iter().copied().filter(|atom| other.atoms.binary_search(atom).is_err()).collect();
        rest.insert(&atoms);
        rest
    }

    /// Changed atoms (`u32::MAX`: every one).
    pub fn count(&self) -> u32 {
        if self.all { u32::MAX } else { self.atoms.len() as u32 }
    }
}

/// What part of a chunk a read looked at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Read {
    /// The text of one of its nodes below its root (the root's whole text
    /// is [`Atom::subtree`]).
    Text = 0,
    /// Its nodes' children.
    Structure = 1,
    /// Its nodes' parents.
    Link = 2,
    /// Its nodes' entries in the names tables (declarations, references,
    /// scopes, effects).
    Names = 3,
}

/// One thing a computation can read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Atom(u64);

impl Atom {
    /// Part `read` of the chunk rooted at `root`.
    pub fn chunk(read: Read, root: u32) -> Atom {
        let part = match read {
            Read::Text => 0,
            Read::Link => 1,
            Read::Names => 2,
            Read::Structure => 4,
        };
        Atom(u64::from(root) << 3 | part)
    }

    /// The whole text beneath chunk root `root` (every chunk below it).
    pub fn subtree(root: u32) -> Atom {
        Atom(u64::from(root) << 3 | 3)
    }

    /// A derived whole-file table's answer for one node (`table` numbers
    /// the table).
    pub fn table(table: u8, node: u32) -> Atom {
        Atom(1 << 62 | u64::from(table) << 32 | u64::from(node))
    }

    /// A derived whole-file table's answers as a whole (`table` numbers it).
    pub fn whole(table: u8) -> Atom {
        Atom(1 << 61 | u64::from(table))
    }

    /// The atom's fingerprint: two atoms share one about once in 2^32.
    fn key(self) -> u32 {
        let mut x = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (x ^ (x >> 31)) as u32
    }
}

/// Which derived table [`Atom::table`] and [`Atom::whole`] name.
pub mod tables {
    /// Narrowing's scan of the guards' tested roots.
    pub const SCAN: u8 = 0;
    /// The module index (imports, exports, globals).
    pub const MODULE: u8 = 1;
    /// Writes replacing a binding.
    pub const WRITTEN: u8 = 2;
    /// Reads of a binding.
    pub const READS: u8 = 3;
    /// A declaration's uses (the names tables' references).
    pub const USES: u8 = 4;
    /// Where the nodes of one kind are (by matcher).
    pub const KINDS: u8 = 5;
    /// A whole-file catalogue: a root attribute's value (by attribute).
    pub const WALLS: u8 = 6;
    /// A value's member by key (by hash of value leaf and key).
    pub const MEMBER_KEYS: u8 = 7;
}

/// The editor's file this thread tracks: its tree (with regions) and text.
#[derive(Clone, Copy)]
struct Tracked {
    tree: *const crate::grammar::Tree,
    text: *const u8,
    len: usize,
}

/// Words of [`Dirty`]'s bloom.
const FILTER: usize = 16;

/// Slots of [`SEEN`].
const SEEN_SLOTS: usize = 64;

thread_local! {
    static READ: Cell<Reading> = const { Cell::new(Reading::NONE) };
    static TRACKED: Cell<Tracked> = const { Cell::new(Tracked { tree: std::ptr::null(), text: std::ptr::null(), len: 0 }) };
    /// Atoms (by slot) the reading of a generation holds already: a walk
    /// reads the same chunk over and over.
    static SEEN: Cell<[(u32, u32); SEEN_SLOTS]> = const { Cell::new([(0, 0); SEEN_SLOTS]) };
    static GENERATIONS: Cell<u32> = const { Cell::new(0) };
}

/// Changes the computation in progress in place.
#[inline]
fn reading<T>(change: impl FnOnce(&mut Reading) -> T) -> T {
    // SAFETY: the cell is this thread's, and nothing else borrows it while
    // `change` (which never reaches the cell) runs.
    READ.with(|read| change(unsafe { &mut *read.as_ptr() }))
}

/// The computation in progress read `footprint` too.
#[inline]
pub fn enter(footprint: &Footprint) {
    if footprint.is_none() { return }
    reading(|read| read.add(footprint));
}

/// The computation in progress read `atom` too.
#[inline]
pub fn enter_atom(atom: Atom) {
    let key = atom.key();
    reading(|read| {
        if read.generation == 0 {
            read.generation = GENERATIONS.with(|count| { count.set(count.get().wrapping_add(1).max(1)); count.get() });
        }
        // SAFETY: as in [`reading`].
        let seen = SEEN.with(|seen| unsafe { &mut (*seen.as_ptr())[key as usize % SEEN_SLOTS] });
        if *seen == (key, read.generation) { return }
        *seen = (key, read.generation);
        read.extend(&[key]);
    });
}

/// The computation in progress read everything.
pub fn enter_all() {
    reading(|read| read.add(&Footprint::all()));
}

/// Starts a computation of its own; [`end`] it with the returned value.
#[inline]
pub fn begin() -> Reading {
    READ.with(|read| read.replace(Reading::NONE))
}

/// Replaces what the computation in progress read.
#[inline]
pub fn set(reading: Reading) {
    READ.with(|read| read.set(reading));
}

/// What the computation in progress read so far.
#[inline]
pub fn current() -> Footprint {
    reading(|read| {
        let mut atoms = read.atoms.clone();
        atoms.sort_unstable();
        atoms.dedup();
        Footprint { atoms, all: read.all, from: read.from, to: read.to, lines_from: read.lines_from, lines_to: read.lines_to }
    })
}

/// Ends the computation [`begin`] started, returning what it read: the
/// work around it read that too.
#[inline]
pub fn end(outer: Reading) -> Footprint {
    let own = READ.with(|read| read.replace(outer)).footprint();
    reading(|read| read.add(&own));
    own
}

/// Ends the computation [`begin`] started when what it read is kept
/// already: the work around it read that too.
#[inline]
pub fn resume(outer: Reading) {
    let own = READ.with(|read| read.replace(outer));
    reading(|read| read.absorb(own));
}

/// Ends the computation [`begin`] started, whose answer is kept behind
/// `atom`: the work around it read only that atom (and nothing, when the
/// computation read nothing of the file).
#[inline]
pub fn resume_behind(outer: Reading, atom: Atom) {
    let own = READ.with(|read| read.replace(outer));
    if own.atoms.is_empty() && !own.all && own.from > own.to && own.lines_from > own.lines_to { return }
    enter_atom(atom);
}

/// Runs `run` without recording its reads anywhere: work whose answer is
/// kept for every reader that records what each reader needs of it.
pub fn quiet<T>(run: impl FnOnce() -> T) -> T {
    let outer = begin();
    let answer = run();
    set(outer);
    answer
}

/// Runs `run` tracking reads of `tree` (an editor's file, see
/// [`crate::grammar::Tree::track`]) whose text is `text`, on this thread.
pub fn tracking<T>(tree: &crate::grammar::Tree, text: &str, run: impl FnOnce() -> T) -> T {
    if tree.regions().is_none() { return run() }
    let outer = TRACKED.with(|tracked| tracked.replace(Tracked { tree, text: text.as_ptr(), len: text.len() }));
    let answer = run();
    TRACKED.with(|tracked| tracked.set(outer));
    answer
}

#[inline]
fn tracked<T>(read: impl FnOnce(&crate::grammar::Tree, &str) -> T) -> Option<T> {
    let tracked = TRACKED.with(Cell::get);
    if tracked.tree.is_null() { return None }
    // SAFETY: set only while `tracking` borrows the tree and its text.
    let (tree, text) = unsafe { (&*tracked.tree, std::str::from_utf8_unchecked(std::slice::from_raw_parts(tracked.text, tracked.len))) };
    Some(read(tree, text))
}

/// Read `text`, the text of node `node`, when it is the tracked file's.
#[inline]
pub fn value_text(node: u32, text: &str) {
    tracked(|tree, source| {
        let start = text.as_ptr() as usize;
        let within = start >= source.as_ptr() as usize && start + text.len() <= source.as_ptr() as usize + source.len();
        let node_text = || tree.nodes.get(node as usize).and_then(|n| source.get(n.start as usize..n.end as usize));
        if within || node_text() == Some(text) { tree.read_text(node) }
    });
}

/// Read the positions of bytes `from..=to` of the tracked file.
#[inline]
pub fn positions(from: usize, to: usize) {
    tracked(|_, _| enter(&Footprint::positions(from as u32, to as u32)));
}

/// Read `text`: when it is a slice of the tracked file's text, what lies
/// there (and where).
#[inline]
pub fn source_text(text: &str) {
    tracked(|_, source| {
        let (at, base) = (text.as_ptr() as usize, source.as_ptr() as usize);
        if at >= base && at + text.len() <= base + source.len() {
            enter(&Footprint::positions((at - base) as u32, (at - base + text.len()) as u32));
        }
    });
}

/// Read the line number of byte `at` of the tracked file.
#[inline]
pub fn line(at: usize) {
    tracked(|_, _| enter(&Footprint::line(at as u32)));
}

/// Read the tracked file's names-table entries of `node`.
#[inline]
pub fn names(node: u32) {
    tracked(|tree, _| if let Some(regions) = tree.regions() { regions.note(Read::Names, node) });
}

/// Read a derived table's answer for the tracked file's `node`.
#[inline]
pub fn table(table: u8, node: u32) {
    tracked(|_, _| enter_atom(Atom::table(table, node)));
}

/// Read a derived table of the tracked file as a whole.
#[inline]
pub fn whole(table: u8) {
    tracked(|_, _| enter_atom(Atom::whole(table)));
}

/// Whether this thread tracks an editor's file.
pub fn is_tracking() -> bool {
    tracked(|_, _| ()).is_some()
}

/// A tree's chunks: each node belongs to its nearest ancestor (or itself)
/// with at least [`Regions::CHUNK`] nodes beneath, the root's chunk holding
/// what no other does.
pub struct Regions {
    chunk: Vec<u32>,
}

const UNREACHED: u32 = u32::MAX;

impl Regions {
    /// Nodes beneath a node that make it a chunk of its own.
    const CHUNK: u32 = 32;
    /// Children that make a chunk a list, and nodes that make one of its
    /// items a chunk of its own.
    const LIST: u32 = 8;
    const ITEM: u32 = 4;

    pub fn new(order: &[u32], len: usize, parent: impl Fn(u32) -> Option<u32>) -> Regions {
        let (mut size, mut children) = (vec![0u32; len], vec![0u32; len]);
        for &id in order.iter().rev() {
            size[id as usize] += 1;
            if let Some(p) = parent(id) {
                size[p as usize] += size[id as usize];
                children[p as usize] += 1;
            }
        }
        let mut chunk = vec![UNREACHED; len];
        for &id in order {
            // Items of a long list (a class's members, a block's statements)
            // are chunks of their own unless tiny: an edit of one leaves the
            // others' reads standing.
            let item = |p: u32| chunk[p as usize] == p && children[p as usize] >= Self::LIST && size[id as usize] >= Self::ITEM;
            chunk[id as usize] = match parent(id) {
                Some(p) if size[id as usize] < Self::CHUNK && !item(p) => chunk[p as usize],
                _ => id,
            };
        }
        Regions { chunk }
    }

    /// The chunk `node` belongs to (`None`: not in the tree).
    pub fn chunk(&self, node: u32) -> Option<u32> {
        self.chunk.get(node as usize).copied().filter(|&c| c != UNREACHED)
    }

    /// What reading part `read` of `node` reads.
    pub fn atom(&self, read: Read, node: u32) -> Footprint {
        match self.chunk(node) {
            Some(chunk) => Footprint::atom(Atom::chunk(read, chunk)),
            None => Footprint::all(),
        }
    }

    #[inline]
    pub fn note(&self, read: Read, node: u32) {
        match self.chunk(node) {
            Some(chunk) => enter_atom(Atom::chunk(read, chunk)),
            None => enter_all(),
        }
    }

    /// Reading the whole text of `node`: a node rooting no chunk holds none
    /// beneath it (a chunk's items are chunks only under a chunk root).
    #[inline]
    pub fn note_subtree(&self, node: u32) {
        match self.chunk(node) {
            Some(chunk) if chunk == node => enter_atom(Atom::subtree(node)),
            Some(chunk) => enter_atom(Atom::chunk(Read::Text, chunk)),
            None => enter_all(),
        }
    }

    /// Every part of the chunk `node` belongs to, and the text beneath it
    /// when it roots one.
    pub fn all_of(&self, node: u32) -> Footprint {
        let mut all = Reading::NONE;
        for read in [Read::Text, Read::Structure, Read::Link, Read::Names] { all.add(&self.atom(read, node)) }
        if self.chunk(node) == Some(node) { all = all.with_atom(Atom::subtree(node)) }
        all.footprint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computations_record_their_own_reads_and_hand_them_outward() {
        let (a, b) = (Footprint::atom(Atom::whole(1)), Footprint::atom(Atom::whole(2)));
        let outer = begin();
        enter(&a);
        let inner = begin();
        enter(&b);
        assert_eq!(end(inner), b);
        assert_eq!(current(), a.with(&b));
        assert_eq!(end(outer), a.with(&b));
        let mut dirty = Dirty::default();
        dirty.add_atom(Atom::whole(2));
        assert!(b.meets(&dirty) && a.with(&b).meets(&dirty));
        assert!(!a.meets(&dirty));
    }

    #[test]
    fn repeated_reads_are_kept_once_and_inner_reads_reach_outer_ones() {
        let outer = begin();
        enter_atom(Atom::whole(1));
        let inner = begin();
        // The inner computation reads what the outer one read already.
        enter_atom(Atom::whole(1));
        for _ in 0..10_000 { enter_atom(Atom::whole(2)) }
        assert_eq!(end(inner).count(), 2);
        assert_eq!(end(outer).count(), 2);
    }

    #[test]
    fn many_atoms_meet_only_their_own() {
        let atoms: Vec<Footprint> = (0..400).map(|node| Footprint::atom(Atom::table(tables::USES, node))).collect();
        let many = atoms.iter().fold(Footprint::NONE, |all, atom| all.with(atom));
        assert_eq!(many.count(), 400);
        let mut dirty = Dirty::default();
        dirty.add_atom(Atom::table(tables::USES, 399));
        assert!(many.meets(&dirty));
        let mut elsewhere = Dirty::default();
        for node in 400..800 { elsewhere.add_atom(Atom::table(tables::USES, node)) }
        assert!(!many.meets(&elsewhere));
        assert!(many.meets(&Dirty::all()) && !Footprint::NONE.meets(&Dirty::all()));
    }

    #[test]
    fn positions_read_across_an_edit_change_and_others_shift() {
        let dirty = Dirty::edit(Edit { start: 10, old_end: 12, new_end: 15, lines: false });
        assert!(!Footprint::line(5).with(&Footprint::line(20)).meets(&dirty));
        assert!(Footprint::line(5).with(&Footprint::line(20)).meets(&Dirty::edit(Edit { lines: true, ..dirty.edit.unwrap() })));
        assert!(Footprint::positions(5, 20).meets(&dirty));
        assert!(!Footprint::positions(0, 9).meets(&dirty));
        let after = Footprint::positions(13, 20);
        assert!(!after.meets(&dirty));
        assert_eq!(after.shifted(&dirty), Footprint::positions(16, 23));
        assert!(!Footprint::NONE.meets(&dirty));
    }

    #[test]
    fn chunks_hold_small_subtrees() {
        // A root with one big child (40 leaves beneath) and one small one.
        let mut parent = vec![None];
        parent.push(Some(0));
        for _ in 0..40 { parent.push(Some(1)) }
        parent.push(Some(0));
        let order: Vec<u32> = (0..parent.len() as u32).collect();
        let regions = Regions::new(&order, parent.len(), |id| parent[id as usize]);
        assert_eq!(regions.chunk(1), Some(1));
        assert_eq!(regions.chunk(5), Some(1));
        assert_eq!(regions.chunk(42), Some(0));
        let subtree = |node| {
            let outer = begin();
            regions.note_subtree(node);
            end(outer)
        };
        assert_eq!(subtree(1), Footprint::atom(Atom::subtree(1)));
        assert_eq!(subtree(42), Footprint::atom(Atom::chunk(Read::Text, 0)));
    }
}
