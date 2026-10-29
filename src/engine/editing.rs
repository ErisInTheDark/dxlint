//! What an editor's lint of a file keeps for its next lint of that file:
//! its tree (so an edit keeps every other node's id), and each rule group's
//! world and diagnostics per node, each with what it read of the file
//! ([`crate::footprint`]): the next lint keeps everything the edit leaves
//! standing.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::diagnostic::Diagnostic;
use super::reads::{self, Reads};
use crate::footprint::{Dirty, Edit, Footprint};
use crate::fx_hash::FxHashMap;
use crate::grammar::names::semantic::State;
use crate::grammar::names::Resolved;
use crate::grammar::Tree;

/// One rule group's world, between two lints of its file.
pub(super) struct WorldCarry {
    pub units: Vec<Arc<super::units::Unit>>,
    pub state: Rc<State>,
    pub interned: crate::grammar::names::question::Interned,
    /// The file's names as the group resolved them.
    pub names: Option<Arc<Resolved>>,
    /// The group's diagnostics by node id.
    pub results: Vec<Option<NodeResult>>,
    /// The file's name conditions' answers ([`crate::expr::AttrMemo`]).
    pub conditions: crate::expr::NameConditions,
    /// The file's walls ([`crate::vm::FileVm`]): attribute, its value's
    /// fingerprint ([`crate::expr::Value::fingerprint`]), and
    /// what computing it read.
    pub walls: Vec<(u16, u64, Footprint)>,
}

/// What a rule group found on one node, and what finding it read.
pub(super) struct NodeResult {
    pub read: Footprint,
    /// The rules applied through node selections ([`super::Linter::check_tree`]).
    pub linked: Vec<usize>,
    pub found: Vec<Diagnostic>,
}

/// One file's last editor lint.
pub(super) struct FileMemory {
    pub text: Arc<str>,
    pub tree: Tree,
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

/// Where `old` and `new` differ, as one edit.
pub(super) fn changed_span(old: &str, new: &str) -> Edit {
    let prefix = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    let room = old.len().min(new.len()) - prefix;
    let suffix = old.bytes().rev().zip(new.bytes().rev()).take(room).take_while(|(a, b)| a == b).count();
    let (old_end, new_end) = (old.len() - suffix, new.len() - suffix);
    let breaks = |text: &[u8]| text.iter().filter(|&&b| b == b'\n' || b == b'\r').count();
    let lines = breaks(&old.as_bytes()[prefix..old_end]) != breaks(&new.as_bytes()[prefix..new_end]);
    Edit { start: prefix as u32, old_end: old_end as u32, new_end: new_end as u32, lines }
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

/// Whether a tree renumbered across edits holds so many empty ids that a
/// fresh lint (fresh numbering) is cheaper to keep.
pub(super) fn sparse(tree: &Tree) -> bool {
    tree.spare_ids() > tree.order.len()
}

/// `found`, moved to the edited text's positions (for diagnostics the edit
/// did not reach: every position lies before or after it). `None` when one
/// lies inside the edit.
pub(super) fn shifted(found: &[Diagnostic], dirty: &Dirty) -> Option<Vec<Diagnostic>> {
    let Some(edit) = dirty.edit else { return Some(found.to_vec()) };
    let moved = |at: usize| {
        let at = u32::try_from(at).ok()?;
        (at <= edit.start || at >= edit.old_end).then(|| edit.moved(at) as usize)
    };
    found.iter().map(|d| {
        let mut d = d.clone();
        d.start = moved(d.start)?;
        d.end = moved(d.end)?;
        for fix in d.fix.iter_mut().chain(d.suggestions.iter_mut().map(|s| &mut s.fix)) {
            fix.range = moved(fix.range.start)?..moved(fix.range.end)?;
            for e in &mut fix.edits {
                e.start = moved(e.start)?;
                e.end = moved(e.end)?;
            }
            for placeholder in &mut fix.placeholders {
                *placeholder = moved(placeholder.start)?..moved(placeholder.end)?;
            }
        }
        Some(d)
    }).collect()
}
