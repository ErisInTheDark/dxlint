//! What an editor's lint of a file keeps for its next lint of that file:
//! its tree (so an edit inside one body keeps every other node's id), and
//! each rule group's world, whose semantic answers outside the edited body
//! stay right.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::reads::{self, Bodies, Reads};
use crate::fx_hash::FxHashMap;
use crate::grammar::names::semantic::State;
use crate::grammar::Tree;

/// What came between two lints of a file: the bodies (bits) it edited and
/// the edited body's byte range in the new text (`None`: no change).
pub(super) type Edited = (u64, Option<(u32, u32)>);

/// One rule group's world, between two lints of its file.
pub(super) struct WorldCarry {
    pub units: Vec<Arc<super::units::Unit>>,
    pub state: Rc<State>,
    pub interned: crate::grammar::names::question::Interned,
}

/// One file's last editor lint.
pub(super) struct FileMemory {
    pub text: Arc<str>,
    pub tree: Tree,
    pub bodies: Option<Arc<Bodies>>,
    /// The linter's revision the lint ran at.
    pub revision: u64,
    /// Per rule group: its world, and what its lints read so far (the
    /// carried answers' reads included).
    pub groups: Vec<Option<(WorldCarry, Arc<Reads>)>>,
}

/// The editor's files' memories, kept with the editor's run.
#[derive(Default)]
pub(super) struct Memories(Mutex<FxHashMap<PathBuf, Kept>>);

/// A memory is only ever used by one thread at a time: taken out of the
/// map, used by the lint (each group's world by its own thread, one after
/// another), and put back. Its `Rc`s never cross threads while shared.
struct Kept(FileMemory);
// SAFETY: see above; nothing in a kept memory is reachable from elsewhere.
unsafe impl Send for Kept {}

impl Memories {
    pub fn take(&self, path: &Path) -> Option<FileMemory> {
        self.0.lock().expect("editor memories").remove(path).map(|kept| kept.0)
    }

    pub fn keep(&self, path: PathBuf, memory: FileMemory) {
        self.0.lock().expect("editor memories").insert(path, Kept(memory));
    }
}

/// A group's carry handed to the thread that runs it.
pub(super) struct Handed(pub Option<(WorldCarry, Arc<Reads>)>);
// SAFETY: moved to exactly one thread, which hands it back the same way.
unsafe impl Send for Handed {}

/// Where `old` and `new` differ: the start of the change, its end in `old`
/// and its end in `new`.
pub(super) fn changed_span(old: &str, new: &str) -> (u32, u32, u32) {
    let prefix = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    let room = old.len().min(new.len()) - prefix;
    let suffix = old.bytes().rev().zip(new.bytes().rev()).take(room).take_while(|(a, b)| a == b).count();
    (prefix as u32, (old.len() - suffix) as u32, (new.len() - suffix) as u32)
}

/// Records again what a carried world's earlier lints read: its kept
/// answers depend on it. The linted file's own reads are each lint's
/// (as are shared answers that read it), never carried.
pub(super) fn replay(previous: &Reads, linted: &Path) {
    for dep in previous.deps.iter().filter(|dep| dep.path() != linted) {
        reads::note(dep.clone());
    }
    for group in previous.groups.iter().filter(|group| !group.deps.iter().any(|dep| dep.path() == linted)) {
        reads::note_group(group);
    }
    for scan in &previous.scans {
        reads::note_scan(scan);
    }
}

/// Whether a carried read set grew past what a lint reads afresh: carrying
/// it further would only slow every check of it.
pub(super) fn overgrown(reads: &Reads) -> bool {
    reads.deps.len() + reads.groups.len() + reads.scans.len() > 100_000
}
