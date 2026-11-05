//! The `.dxcache`s of a batch of server lints. A [`Cache`] judges each saved
//! read once, for one moment in time; a server's batches outlive edits, so
//! this owner reopens a cache once files changed, and saves as it goes: a
//! batch may never end.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::engine::cache::Cache;
use crate::engine::Linter;

/// How much new work closing the editor can lose.
const SAVE_EVERY: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct LiveCaches {
    open: HashMap<PathBuf, Live>,
}

struct Live {
    cache: Cache,
    revision: u64,
    saved_at: Instant,
}

impl LiveCaches {
    /// `linter`'s cache, true of the files as they are now.
    pub fn of(&mut self, linter: &Linter) -> &Cache {
        self.of_at(linter, Instant::now())
    }

    fn of_at(&mut self, linter: &Linter, now: Instant) -> &Cache {
        let revision = linter.revision();
        let stale = self.open.get(&linter.root).is_some_and(|live| {
            live.revision != revision || live.cache.has_new() && now.duration_since(live.saved_at) >= SAVE_EVERY
        });
        if stale {
            if let Some(live) = self.open.remove(&linter.root) { let _ = live.cache.save(false); }
        }
        &self.open.entry(linter.root.clone()).or_insert_with(|| Live { cache: Cache::open(linter), revision, saved_at: now }).cache
    }

    /// Keeps what the batch found, however far it got.
    pub fn save(&mut self) {
        for (_, live) in self.open.drain() {
            let _ = live.cache.save(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::reads::{self, Dep};
    use std::sync::Arc;

    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join("dxlint-live-cache-tests").join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("tmp dir");
        root
    }

    #[test]
    fn a_changed_file_is_not_answered_from_before_the_change() {
        let root = project("edits");
        let read = root.join("read.t");
        std::fs::write(&read, "before").expect("write");
        let linter = crate::engine::tests::linter_at(&root, "");
        let hash = reads::read_unrecorded(&read).1;
        let opened = Cache::open(&linter);
        let ((), reading) = reads::track(|| reads::note(Dep::Text(reads::dep_path(&read), hash)));
        opened.store("a.t", "text", &[], Arc::new(reading));
        opened.save(false).expect("save");

        let mut live = LiveCaches::default();
        assert!(live.of(&linter).lookup(&linter, "a.t", "text").is_some());
        std::fs::write(&read, "after").expect("write");
        linter.invalidate(std::slice::from_ref(&read));
        assert!(live.of(&linter).lookup(&linter, "a.t", "text").is_none());
    }

    #[test]
    fn new_results_reach_the_disk_before_the_batch_ends() {
        let root = project("periodic");
        let linter = crate::engine::tests::linter_at(&root, "");
        let start = Instant::now();
        let mut live = LiveCaches::default();
        live.of_at(&linter, start).store("a.t", "text", &[], Arc::default());
        let saved = |linter: &Linter| Cache::open(linter).lookup(linter, "a.t", "text").is_some();
        live.of_at(&linter, start);
        assert!(!saved(&linter));
        live.of_at(&linter, start + SAVE_EVERY);
        assert!(saved(&linter));
    }
}
