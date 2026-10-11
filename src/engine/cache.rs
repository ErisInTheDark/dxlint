//! `.zlintcache`: lint results kept between runs, beside the config. A
//! file's result is reused when its text and everything its lint read
//! (other files' summaries, module resolution probes, tsconfigs: see
//! `engine::reads`) still match. Results are kept for clean files and
//! for files with problems alike, so a warm run only lints what changed.
//!
//! The whole cache belongs to one rule state: the config's tokens (imports
//! and macros expanded), the project root and the zlint executable. Any
//! change there starts afresh.
//!
//! Reads shared by many files (a tsconfig's globals, resolved member names)
//! are stored once as groups, and every dep and group is checked at most
//! once per run.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use super::{Diagnostic, Edit, Fix, Linter};
use crate::config::ast::Severity;
use super::reads::{hash_bytes, Dep, Reads};

pub const FILE_NAME: &str = ".zlintcache";
/// Bumped whenever the encoding changes.
const MAGIC: &[u8; 4] = b"ZLC4";

/// Deps and groups, by index into the tables.
#[derive(Default, Clone, PartialEq, Eq, Hash)]
struct Refs {
    deps: Vec<u32>,
    groups: Vec<u32>,
}

struct Entry {
    content: u64,
    refs: Refs,
    diags: Vec<Diagnostic>,
}

/// A result this run produced or confirmed.
enum Fresh {
    /// The saved entry still holds.
    Kept,
    New { content: u64, reads: Arc<Reads>, diags: Vec<Diagnostic> },
}

/// What a saved cache holds.
#[derive(Default)]
struct Saved {
    deps: Vec<Dep>,
    /// A group's refs only point at earlier groups.
    groups: Vec<Refs>,
    entries: HashMap<String, Entry>,
}

pub struct Cache {
    path: PathBuf,
    root: PathBuf,
    key: u64,
    saved: Saved,
    /// Whether each saved dep and group still holds, checked once per run.
    dep_holds: Vec<OnceLock<bool>>,
    group_holds: Vec<OnceLock<bool>>,
    /// Saved groups as reads, for results taken from the cache.
    group_reads: Vec<OnceLock<Arc<Reads>>>,
    fresh: Mutex<HashMap<String, Fresh>>,
    /// Something new to write.
    changed: AtomicBool,
    hits: AtomicUsize,
}

impl Cache {
    /// The cache for `linter`'s rules: what was saved, if it was saved for
    /// exactly these rules and this zlint, else an empty one.
    pub fn open(linter: &Linter) -> Cache {
        let path = linter.root.join(FILE_NAME);
        let key = key(linter.fingerprint);
        let saved = std::fs::read(&path).ok().and_then(|bytes| decode(&bytes, key, &linter.root)).unwrap_or_default();
        Cache {
            path,
            root: linter.root.clone(),
            key,
            dep_holds: saved.deps.iter().map(|_| OnceLock::new()).collect(),
            group_holds: saved.groups.iter().map(|_| OnceLock::new()).collect(),
            group_reads: saved.groups.iter().map(|_| OnceLock::new()).collect(),
            saved,
            fresh: Mutex::default(),
            changed: AtomicBool::new(false),
            hits: AtomicUsize::new(0),
        }
    }

    /// Files whose results came from the cache this run.
    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    /// The saved result for `rel` (a path relative to the config, `/`
    /// separated), if `text` and everything its lint read are unchanged,
    /// with what that lint read.
    pub fn lookup(&self, linter: &Linter, rel: &str, text: &str) -> Option<(Vec<Diagnostic>, Reads)> {
        let entry = self.saved.entries.get(rel)?;
        if entry.content != hash_bytes(text.as_bytes()) || !self.holds(linter, &entry.refs) {
            return None;
        }
        self.hits.fetch_add(1, Ordering::Relaxed);
        self.fresh.lock().expect("cache lock").insert(rel.to_string(), Fresh::Kept);
        Some((entry.diags.clone(), self.reads(&entry.refs)))
    }

    fn holds(&self, linter: &Linter, refs: &Refs) -> bool {
        refs.deps.iter().all(|&i| *self.dep_holds[i as usize].get_or_init(|| linter.holds(&self.saved.deps[i as usize])))
            && refs.groups.iter().all(|&i| *self.group_holds[i as usize].get_or_init(|| self.holds(linter, &self.saved.groups[i as usize])))
    }

    /// Saved refs as reads, each group built once.
    fn reads(&self, refs: &Refs) -> Reads {
        Reads {
            deps: refs.deps.iter().map(|&i| self.saved.deps[i as usize].clone()).collect(),
            groups: refs.groups.iter().map(|&i| self.group_reads[i as usize].get_or_init(|| Arc::new(self.reads(&self.saved.groups[i as usize]))).clone()).collect(),
        }
    }

    /// Remembers this run's result for `rel`.
    pub fn store(&self, rel: &str, text: &str, diags: &[Diagnostic], reads: Arc<Reads>) {
        let fresh = Fresh::New { content: hash_bytes(text.as_bytes()), reads, diags: diags.to_vec() };
        self.fresh.lock().expect("cache lock").insert(rel.to_string(), fresh);
        self.changed.store(true, Ordering::Relaxed);
    }

    /// Writes the cache: this run's results, plus (unless `prune`, for a
    /// run that saw every file) older results for files not seen this run.
    /// Nothing is written when nothing changed.
    pub fn save(self, prune: bool) -> io::Result<()> {
        let fresh = self.fresh.into_inner().unwrap_or_default();
        let pruned = prune && self.saved.entries.keys().any(|k| !fresh.contains_key(k));
        if !self.changed.load(Ordering::Relaxed) && !pruned {
            return Ok(());
        }
        let mut tables = Tables::new(&self.saved);
        let mut entries: Vec<(&str, u64, Refs, &[Diagnostic])> = Vec::new();
        for (rel, f) in &fresh {
            entries.push(match f {
                Fresh::New { content, reads, diags } => (rel, *content, tables.reads(reads), diags),
                Fresh::Kept => {
                    let e = &self.saved.entries[rel];
                    (rel, e.content, tables.saved_refs(&e.refs), &e.diags)
                }
            });
        }
        if !prune {
            for (rel, e) in self.saved.entries.iter().filter(|(k, _)| !fresh.contains_key(*k)) {
                entries.push((rel, e.content, tables.saved_refs(&e.refs), &e.diags));
            }
        }
        entries.sort_unstable_by(|a, b| a.0.cmp(b.0));

        let mut w = Writer { out: Vec::new(), root: &self.root, strings: Vec::new(), string_ids: HashMap::new() };
        w.var(tables.deps.len() as u64);
        for d in &tables.deps {
            w.dep(d);
        }
        w.var(tables.groups.len() as u64);
        for g in &tables.groups {
            w.refs(g);
        }
        w.var(entries.len() as u64);
        for (rel, content, refs, diags) in entries {
            w.str(rel);
            w.out.extend_from_slice(&content.to_le_bytes());
            w.refs(&refs);
            w.var(diags.len() as u64);
            for d in diags {
                w.diag(d);
            }
        }
        // Written aside and renamed over, so a reader never sees half a file.
        let tmp = self.path.with_extension(format!("tmp{}", std::process::id()));
        std::fs::write(&tmp, w.finish(self.key))?;
        std::fs::rename(&tmp, &self.path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }
}

/// The rule state the cache belongs to.
fn key(fingerprint: u64) -> u64 {
    static EXE: OnceLock<u64> = OnceLock::new();
    // A zlint that cannot read itself never reuses anything.
    let exe = *EXE.get_or_init(|| {
        std::env::current_exe().and_then(std::fs::read).map_or_else(|_| std::process::id().into(), |bytes| hash_bytes(&bytes))
    });
    hash_bytes(&[MAGIC.as_slice(), &fingerprint.to_le_bytes(), &exe.to_le_bytes()].concat())
}

/// The tables being written: deps and groups interned, groups by content.
struct Tables<'a> {
    saved: &'a Saved,
    deps: Vec<Dep>,
    dep_ids: HashMap<Dep, u32>,
    groups: Vec<Refs>,
    group_ids: HashMap<Refs, u32>,
    /// Groups already interned, by address (alive as long as the run's
    /// results) or by saved index.
    by_address: HashMap<usize, u32>,
    by_saved: HashMap<u32, u32>,
}

impl<'a> Tables<'a> {
    fn new(saved: &'a Saved) -> Self {
        Tables {
            saved,
            deps: Vec::new(),
            dep_ids: HashMap::new(),
            groups: Vec::new(),
            group_ids: HashMap::new(),
            by_address: HashMap::new(),
            by_saved: HashMap::new(),
        }
    }

    fn dep(&mut self, dep: &Dep) -> u32 {
        if let Some(&id) = self.dep_ids.get(dep) {
            return id;
        }
        let id = self.deps.len() as u32;
        self.deps.push(dep.clone());
        self.dep_ids.insert(dep.clone(), id);
        id
    }

    fn group(&mut self, refs: Refs) -> u32 {
        if let Some(&id) = self.group_ids.get(&refs) {
            return id;
        }
        let id = self.groups.len() as u32;
        self.groups.push(refs.clone());
        self.group_ids.insert(refs, id);
        id
    }

    fn reads(&mut self, reads: &Reads) -> Refs {
        let deps = reads.deps.iter().map(|d| self.dep(d)).collect();
        let groups = reads.groups.iter().map(|g| self.reads_group(g)).collect();
        sorted(Refs { deps, groups })
    }

    fn reads_group(&mut self, g: &Arc<Reads>) -> u32 {
        let address = Arc::as_ptr(g) as usize;
        if let Some(&id) = self.by_address.get(&address) {
            return id;
        }
        let refs = self.reads(g);
        let id = self.group(refs);
        self.by_address.insert(address, id);
        id
    }

    fn saved_refs(&mut self, refs: &Refs) -> Refs {
        let saved = self.saved;
        let deps = refs.deps.iter().map(|&i| self.dep(&saved.deps[i as usize])).collect();
        let groups = refs.groups.iter().map(|&i| self.saved_group(i)).collect();
        sorted(Refs { deps, groups })
    }

    fn saved_group(&mut self, i: u32) -> u32 {
        if let Some(&id) = self.by_saved.get(&i) {
            return id;
        }
        let refs = self.saved_refs(&self.saved.groups[i as usize].clone());
        let id = self.group(refs);
        self.by_saved.insert(i, id);
        id
    }
}

fn sorted(mut refs: Refs) -> Refs {
    refs.deps.sort_unstable();
    refs.deps.dedup();
    refs.groups.sort_unstable();
    refs.groups.dedup();
    refs
}

// ---- encoding ------------------------------------------------------------------

const SEVERITIES: [Severity; 5] = [Severity::Off, Severity::Hint, Severity::Info, Severity::Warn, Severity::Error];

/// Encodes the body, then [`Writer::finish`] puts the header and string
/// table in front: every string (rule, message, directory, file name) is
/// stored once.
struct Writer<'a> {
    out: Vec<u8>,
    root: &'a Path,
    strings: Vec<String>,
    string_ids: HashMap<String, u32>,
}

fn write_var(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

impl Writer<'_> {
    fn finish(self, key: u64) -> Vec<u8> {
        let mut file = Vec::with_capacity(self.out.len() + 64 * self.strings.len());
        file.extend_from_slice(MAGIC);
        file.extend_from_slice(&key.to_le_bytes());
        write_var(&mut file, self.strings.len() as u64);
        for s in &self.strings {
            write_var(&mut file, s.len() as u64);
            file.extend_from_slice(s.as_bytes());
        }
        file.extend_from_slice(&self.out);
        file
    }

    fn var(&mut self, v: u64) {
        write_var(&mut self.out, v);
    }

    fn str(&mut self, s: &str) {
        let id = match self.string_ids.get(s) {
            Some(&id) => id,
            None => {
                let id = self.strings.len() as u32;
                self.strings.push(s.to_string());
                self.string_ids.insert(s.to_string(), id);
                id
            }
        };
        self.var(u64::from(id));
    }

    /// A directory and a name; paths below the root are kept relative.
    fn path(&mut self, p: &Path) {
        let (flag, p) = match p.strip_prefix(self.root) {
            Ok(rel) => (0, rel),
            Err(_) => (1, p),
        };
        self.out.push(flag);
        match (p.parent(), p.file_name()) {
            (Some(dir), Some(name)) => {
                self.str(&dir.to_string_lossy());
                self.str(&name.to_string_lossy());
            }
            _ => {
                self.str(&p.to_string_lossy());
                self.str("");
            }
        }
    }

    /// Sorted ids, as gaps (mostly one byte each).
    fn ids(&mut self, ids: &[u32]) {
        self.var(ids.len() as u64);
        let mut last = 0;
        for &id in ids {
            self.var(u64::from(id - last));
            last = id;
        }
    }

    fn refs(&mut self, refs: &Refs) {
        self.ids(&refs.deps);
        self.ids(&refs.groups);
    }

    fn dep(&mut self, d: &Dep) {
        let (tag, a, b) = match d {
            Dep::Text(_, h) => (0, *h, None),
            Dep::IsFile(_, found) => (1, u64::from(*found), None),
            Dep::IsDir(_, found) => (2, u64::from(*found), None),
            Dep::List(_, h) => (3, *h, None),
            Dep::Summary(_, content, fingerprint) => (4, *content, Some(*fingerprint)),
            Dep::Discovered(_, _, h) => (5, *h, None),
        };
        self.out.push(tag);
        self.path(d.path());
        self.out.extend_from_slice(&a.to_le_bytes());
        if let Some(b) = b {
            self.out.extend_from_slice(&b.to_le_bytes());
        }
        if let Dep::Discovered(name, ..) = d {
            self.str(name);
        }
    }

    fn diag(&mut self, d: &Diagnostic) {
        self.str(&d.rule);
        self.out.push(SEVERITIES.iter().position(|s| *s == d.severity).unwrap_or(0) as u8);
        self.str(&d.message);
        self.var(d.message_index.map_or(0, |index| index as u64 + 1));
        self.var(d.start as u64);
        self.var(d.end as u64);
        match &d.fix {
            None => self.var(0),
            Some(fix) => {
                self.var(fix.edits.len() as u64 + 1);
                self.var(fix.range.start as u64);
                self.var(fix.range.end as u64);
                for e in &fix.edits {
                    self.var(e.start as u64);
                    self.var(e.end as u64);
                    self.str(&e.text);
                }
            }
        }
    }
}

fn decode(bytes: &[u8], key: u64, root: &Path) -> Option<Saved> {
    let mut r = Reader { bytes, at: 0, root, strings: Vec::new() };
    if r.take(4)? != MAGIC || r.u64()? != key {
        return None;
    }
    for _ in 0..r.var()? {
        let n = usize::try_from(r.var()?).ok()?;
        let s = String::from_utf8(r.take(n)?.to_vec()).ok()?;
        r.strings.push(s);
    }
    let deps = (0..r.var()?).map(|_| r.dep()).collect::<Option<Vec<_>>>()?;
    let mut groups = Vec::new();
    for _ in 0..r.var()? {
        let refs = r.refs(deps.len(), groups.len())?;
        groups.push(refs);
    }
    let mut entries = HashMap::new();
    for _ in 0..r.var()? {
        let rel = r.str()?;
        let content = r.u64()?;
        let refs = r.refs(deps.len(), groups.len())?;
        let diags = (0..r.var()?).map(|_| r.diag()).collect::<Option<Vec<_>>>()?;
        entries.insert(rel, Entry { content, refs, diags });
    }
    Some(Saved { deps, groups, entries })
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    root: &'a Path,
    strings: Vec<String>,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let out = self.bytes.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(out)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn var(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            v |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    fn str(&mut self) -> Option<String> {
        let id = usize::try_from(self.var()?).ok()?;
        self.strings.get(id).cloned()
    }

    /// Sorted ids below `limit`, written as gaps.
    fn ids(&mut self, limit: usize) -> Option<Vec<u32>> {
        let mut last = 0u64;
        let n = self.var()?;
        let mut out = Vec::new();
        for _ in 0..n {
            last = last.checked_add(self.var()?)?;
            if last >= limit as u64 {
                return None;
            }
            out.push(last as u32);
        }
        Some(out)
    }

    fn refs(&mut self, deps: usize, groups: usize) -> Option<Refs> {
        Some(Refs { deps: self.ids(deps)?, groups: self.ids(groups)? })
    }

    fn path(&mut self) -> Option<PathBuf> {
        let flag = self.u8()?;
        let (dir, name) = (self.str()?, self.str()?);
        let mut p = match flag {
            0 => self.root.join(dir),
            1 => PathBuf::from(dir),
            _ => return None,
        };
        if !name.is_empty() {
            p.push(name);
        }
        Some(p)
    }

    fn dep(&mut self) -> Option<Dep> {
        let tag = self.u8()?;
        let path = self.path()?;
        let a = self.u64()?;
        Some(match tag {
            0 => Dep::Text(path, a),
            1 => Dep::IsFile(path, a != 0),
            2 => Dep::IsDir(path, a != 0),
            3 => Dep::List(path, a),
            4 => Dep::Summary(path, a, self.u64()?),
            // An unknown discovery (another build's front end) drops the entry.
            5 => Dep::Discovered(super::reads::discovery_name(&self.str()?)?, path, a),
            _ => return None,
        })
    }

    fn diag(&mut self) -> Option<Diagnostic> {
        let rule = self.str()?;
        let severity = *SEVERITIES.get(self.u8()? as usize)?;
        let message = self.str()?;
        let message_index = self.var()?.checked_sub(1).map(|index| index as usize);
        let start = self.var()? as usize;
        let end = self.var()? as usize;
        let fix = match self.var()? {
            0 => None,
            n => {
                let range = self.var()? as usize..self.var()? as usize;
                let edits = (1..n)
                    .map(|_| Some(Edit { start: self.var()? as usize, end: self.var()? as usize, text: self.str()? }))
                    .collect::<Option<Vec<_>>>()?;
                Some(Fix { range, edits })
            }
        };
        Some(Diagnostic { rule, severity, message, message_index, start, end, fix })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_fixes_keep_declared_range_priority() {
        let root = Path::new("/root");
        let linter = crate::engine::tests::linter(r#"
            in .t match outer /a/ inner:/b/ /c/ {
                rule outer warn "outer" fix as "abX"
            }
            in outer.inner rule inner warn "inner" fix as "B"
        "#);
        let diagnostics = linter.lint(&root.join("file.t"), "abc");
        let mut writer = Writer { out: Vec::new(), root, strings: Vec::new(), string_ids: HashMap::new() };
        for diagnostic in &diagnostics {
            writer.diag(diagnostic);
        }
        let mut reader = Reader { bytes: &writer.out, at: 0, root, strings: writer.strings };
        let cached: Vec<_> = (0..diagnostics.len()).map(|_| reader.diag().expect("cached diagnostic")).collect();
        assert_eq!(crate::engine::apply_fixes("abc", &cached), ("abX".into(), 1));
    }
}
