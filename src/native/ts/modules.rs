//! The project around the linted files: file summaries (read once, shared
//! across threads), module resolution (relative paths, tsconfig `paths`,
//! `node_modules`, `declare module "x"`) and each tsconfig's global scope
//! (its `lib` files and `@types` packages).
//!
//! Every filesystem read goes through [`reads`], and every shared answer
//! (summaries, tsconfigs, globals, member names) keeps what it was computed
//! from, so a lint's recorded deps cover what it learned from other lints.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use super::reads::{self, Cached, Dep, Reads, MISSING};
use super::summary::{summarise, Export, FileSummary, Module, Scope, Summary};
use super::view::Raw;
use crate::config::source::normalize_path;

/// Re-exports followed before giving up (cycles, very deep barrels).
const MAX_HOPS: usize = 16;

#[cfg(test)]
thread_local! {
    static RESOLUTION_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

/// A declaration somewhere in the project.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeclRef {
    pub file: FileId,
    pub decl: u32,
}

/// An intermediate binding traversed while resolving an exported name.
#[derive(Debug, Clone)]
pub struct ExportAlias {
    pub file: FileId,
    pub name: Box<str>,
    pub module: Option<Box<str>>,
    pub doc: Option<Box<str>>,
}

#[derive(Debug, Clone, Default)]
pub struct ExportResolution {
    pub declarations: Vec<DeclRef>,
    pub aliases: Vec<ExportAlias>,
}

impl ExportResolution {
    fn extend(&mut self, other: Self) {
        self.declarations.extend(other.declarations);
        self.aliases.extend(other.aliases);
    }
}

/// Where a module specifier leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModRef {
    File(FileId),
    /// `declare module "x"` number `index` in a file.
    Ambient(FileId, usize),
}

/// What a tsconfig says about types.
#[derive(Debug, Default)]
struct Config {
    dir: PathBuf,
    /// tsconfig `paths`: patterns and targets, relative to `paths_base`.
    paths: Vec<(String, Vec<String>)>,
    paths_base: PathBuf,
    lib: Vec<String>,
    types: Option<Vec<String>>,
    /// Directories of referenced projects (`references`), whose
    /// declarations are part of this program.
    references: Vec<PathBuf>,
    /// Boolean `compilerOptions` (`strict`, `noImplicitThis`, ...), the
    /// nearest `extends` level winning.
    flags: HashMap<String, bool>,
    node_module: Option<bool>,
}

/// Options `strict` turns on unless they are set themselves.
const STRICT_FAMILY: &[&str] = &[
    "alwaysStrict", "noImplicitAny", "noImplicitThis", "strictBindCallApply", "strictBuiltinIteratorReturn",
    "strictFunctionTypes", "strictNullChecks", "strictPropertyInitialization", "useUnknownInCatchVariables",
];

/// Everything visible without an import, for one tsconfig.
#[derive(Default)]
pub struct Globals {
    /// Unique per build (0: the empty scope of files without a tsconfig).
    pub id: u64,
    pub names: HashMap<Box<str>, Vec<DeclRef>>,
    pub modules: HashMap<Box<str>, Vec<ModRef>>,
}

// TypeScript's libMap aliases name ES-only libraries, not full-target wrappers.
fn canonical_library(name: &str) -> &str {
    const ALIASES: &[(&str, &str)] = &[
        ("es6", "es2015"), ("es7", "es2016"),
        ("esnext.asynciterable", "es2018.asynciterable"), ("esnext.symbol", "es2019.symbol"),
        ("esnext.bigint", "es2020.bigint"), ("esnext.weakref", "es2021.weakref"),
        ("esnext.object", "es2024.object"), ("esnext.regexp", "es2024.regexp"),
        ("esnext.string", "es2024.string"), ("esnext.float16", "es2025.float16"),
        ("esnext.iterator", "es2025.iterator"), ("esnext.promise", "es2025.promise"),
    ];
    ALIASES.iter().find_map(|&(alias, canonical)| (alias == name).then_some(canonical)).unwrap_or(name)
}

#[derive(Default)]
struct Files {
    by_path: HashMap<PathBuf, FileId>,
    paths: Vec<PathBuf>,
}

/// A file's summary, with the content hash it was built from and its
/// fingerprint ([`MISSING`] for both when the file has none).
#[derive(Clone)]
struct Known {
    summary: Option<Summary>,
    content: u64,
    fingerprint: u64,
}

#[derive(Default)]
pub struct Project {
    discovery_jobs: usize,
    files: RwLock<Files>,
    summaries: RwLock<HashMap<FileId, Known>>,
    configs: Mutex<HashMap<PathBuf, Cached<Option<Arc<Config>>>>>,
    /// Built once per distinct scope, by whichever lint asks first.
    globals: Mutex<HashMap<String, Arc<GlobalsCell>>>,
    /// Member names of named types (declarations and printed arguments):
    /// the same few types are asked about from many files.
    members: RwLock<HashMap<MembersKey, Cached<MemberNames>>>,
    /// Module specifiers resolved from an importing file, misses included.
    resolutions: RwLock<HashMap<ResolutionKey, Cached<Option<ModRef>>>>,
}

/// `(importing file, specifier as written)`.
type ResolutionKey = (FileId, Box<str>);

pub type MemberNames = Arc<[Box<str>]>;
type GlobalsCell = OnceLock<Cached<Arc<Globals>>>;

/// `(globals id, declarations, printed type arguments)`: heritage resolves
/// through a tsconfig's globals, so the same declaration can have different
/// inherited members under different `lib` settings.
pub type MembersKey = (u64, Vec<DeclRef>, Vec<String>);

/// What changed, from [`Project::invalidate`].
pub struct Stale {
    changed: Vec<PathBuf>,
    /// Directories whose global declaration files changed.
    globals: Vec<PathBuf>,
    /// Touched deps already checked.
    checked: Mutex<HashMap<Dep, bool>>,
    /// Groups already checked, by address (kept alive, so no address is
    /// reused meanwhile).
    groups: Mutex<HashMap<usize, (Arc<Reads>, bool)>>,
}

static GLOBALS_BUILT: AtomicU64 = AtomicU64::new(1);

impl Project {
    pub fn set_discovery_jobs(&mut self, jobs: usize) {
        self.discovery_jobs = jobs.max(1);
    }

    pub fn file_id(&self, path: &Path) -> FileId {
        if let Some(&id) = self.files.read().ok().and_then(|f| f.by_path.get(path).copied()).as_ref() {
            return id;
        }
        let mut files = self.files.write().expect("files lock");
        if let Some(&id) = files.by_path.get(path) {
            return id;
        }
        let id = FileId(files.paths.len() as u32);
        files.paths.push(path.to_path_buf());
        files.by_path.insert(path.to_path_buf(), id);
        id
    }

    pub fn path(&self, id: FileId) -> PathBuf {
        self.files.read().expect("files lock").paths[id.0 as usize].clone()
    }

    fn existing_id(&self, path: &Path) -> Option<FileId> {
        self.files.read().ok()?.by_path.get(path).copied()
    }

    /// A file's summary, reading and parsing it the first time.
    pub fn summary(&self, id: FileId) -> Option<Summary> {
        let path = self.path(id);
        let known = self.known(id, &path);
        reads::note(Dep::Summary(path, known.content, known.fingerprint));
        known.summary
    }

    fn known(&self, id: FileId, path: &Path) -> Known {
        if let Some(k) = self.summaries.read().ok().and_then(|m| m.get(&id).cloned()) {
            return k;
        }
        let (text, content) = reads::read_unrecorded(path);
        let summary = text.and_then(|src| summary_of(path, &src));
        let fingerprint = summary.as_ref().map_or(MISSING, |s| s.fingerprint());
        let known = Known { summary, content, fingerprint };
        match self.summaries.write() {
            // A file being linted may have registered its own meanwhile.
            Ok(mut map) => map.entry(id).or_insert(known).clone(),
            Err(_) => known,
        }
    }

    /// Registers the summary of a file being linted, from its `text` (which
    /// may differ from the disk's).
    pub fn summary_for(&self, path: &Path, text: &str, view: &impl super::view::View, root: u32) -> (FileId, Summary) {
        let id = self.file_id(path);
        let dts = [".d.ts", ".d.mts", ".d.cts"].iter().any(|suffix| path.to_string_lossy().ends_with(suffix));
        let s: Summary = Arc::new(summarise(view, root, dts));
        let known = Known { summary: Some(s.clone()), content: reads::hash_bytes(text.as_bytes()), fingerprint: s.fingerprint() };
        if let Ok(mut map) = self.summaries.write() {
            map.insert(id, known);
        }
        (id, s)
    }

    /// The member names under `key`, computed by `compute` the first time.
    /// `asker` is the file computing them: its checker reads its own
    /// syntax, not just its summary, so the answer depends on its text.
    pub fn members_or(&self, key: MembersKey, asker: FileId, compute: impl FnOnce() -> MemberNames) -> MemberNames {
        if let Some(c) = self.members.read().ok().and_then(|m| m.get(&key).cloned()) {
            return c.get();
        }
        let cached = Cached::compute(|| {
            let path = self.path(asker);
            reads::note(Dep::Text(path.clone(), self.known(asker, &path).content));
            compute()
        });
        if let Ok(mut m) = self.members.write() {
            m.insert(key, cached.clone());
        }
        cached.get()
    }

    // ---- staleness -----------------------------------------------------------

    /// Whether `dep` still holds.
    pub fn holds(&self, dep: &Dep) -> bool {
        dep.holds(|path| self.known(self.file_id(path), path).fingerprint)
    }

    /// Forgets what the `changed` files (edited, created or deleted) said:
    /// their summaries and every shared answer computed from them. The
    /// result tells which recorded reads no longer hold
    /// ([`Project::is_stale`]).
    pub fn invalidate(&self, changed: &[PathBuf]) -> Stale {
        if let Ok(mut map) = self.summaries.write() {
            for id in changed.iter().filter_map(|p| self.existing_id(p)) {
                map.remove(&id);
            }
        }
        let stale = Stale {
            changed: changed.to_vec(),
            globals: reads::globals_changed(changed),
            checked: Mutex::default(),
            groups: Mutex::default(),
        };
        if let Ok(mut configs) = self.configs.lock() {
            configs.retain(|_, c| !self.is_stale(&stale, &c.reads));
        }
        if let Ok(mut globals) = self.globals.lock() {
            globals.retain(|_, cell| cell.get().is_none_or(|c| !self.is_stale(&stale, &c.reads)));
        }
        let live: HashSet<u64> = self.globals.lock().map(|g| g.values().filter_map(|c| c.get().map(|c| c.value.id)).collect()).unwrap_or_default();
        if let Ok(mut members) = self.members.write() {
            members.retain(|key, c| (key.0 == 0 || live.contains(&key.0)) && !self.is_stale(&stale, &c.reads));
        }
        if let Ok(mut resolutions) = self.resolutions.write() {
            resolutions.retain(|_, answer| !self.is_stale(&stale, &answer.reads));
        }
        stale
    }

    /// Whether anything in `reads` (recorded by a lint, or by a shared
    /// answer) no longer holds after the changes in `stale`.
    pub fn is_stale(&self, stale: &Stale, reads: &Reads) -> bool {
        reads.deps.iter().any(|dep| self.dep_is_stale(stale, dep))
            || reads.groups.iter().any(|g| {
                let key = Arc::as_ptr(g) as usize;
                let checked = stale.groups.lock().ok().and_then(|c| c.get(&key).map(|(_, gone)| *gone));
                checked.unwrap_or_else(|| {
                    let gone = self.is_stale(stale, g);
                    if let Ok(mut c) = stale.groups.lock() {
                        c.insert(key, (g.clone(), gone));
                    }
                    gone
                })
            })
    }

    fn dep_is_stale(&self, stale: &Stale, dep: &Dep) -> bool {
        if !stale.changed.iter().any(|p| dep.touches(p)) {
            return false;
        }
        if let Dep::Discovered(name, dir, _) = dep {
            if *name == reads::GLOBAL_FILES.name {
                return stale.globals.contains(dir);
            }
        }
        let checked = stale.checked.lock().ok().and_then(|c| c.get(dep).copied());
        checked.unwrap_or_else(|| {
            let gone = !self.holds(dep);
            if let Ok(mut c) = stale.checked.lock() {
                c.insert(dep.clone(), gone);
            }
            gone
        })
    }

    // ---- modules -----------------------------------------------------------

    /// Where `spec`, imported from `from`, leads.
    pub fn resolve(&self, from: FileId, spec: &str) -> Option<ModRef> {
        let key = (from, spec.into());
        if let Some(answer) = self.resolutions.read().ok().and_then(|map| map.get(&key).cloned()) {
            return answer.get();
        }
        // Resolution may build globals and consult other project answers.
        // Keep the cache unlocked while computing, including cached misses.
        let answer = Cached::compute(|| self.resolve_uncached(from, spec));
        if let Ok(mut map) = self.resolutions.write() { map.insert(key, answer.clone()); }
        answer.get()
    }

    fn resolve_uncached(&self, from: FileId, spec: &str) -> Option<ModRef> {
        #[cfg(test)]
        RESOLUTION_WORK.with(|work| work.set(work.get() + 1));
        let from_path = self.path(from);
        let dir = from_path.parent()?;
        if spec.starts_with("./") || spec.starts_with("../") || spec == "." || spec == ".." {
            return with_extension(&normalize_path(&dir.join(spec))).map(|p| ModRef::File(self.file_id(&p)));
        }
        let config = self.config_for(dir);
        if let Some(config) = &config {
            for (pattern, targets) in &config.paths {
                let captured = match pattern.split_once('*') {
                    Some((pre, post)) => spec.strip_prefix(pre).and_then(|r| r.strip_suffix(post)),
                    None => (spec == pattern).then_some(""),
                };
                let Some(captured) = captured else { continue };
                for t in targets {
                    let target = normalize_path(&config.paths_base.join(t.replace('*', captured)));
                    if let Some(found) = with_extension(&target).or_else(|| package_types(&target)) {
                        return Some(ModRef::File(self.file_id(&found)));
                    }
                }
            }
        }
        if let Some(found) = config.as_ref().and_then(|c| self.globals_of(c).modules.get(spec).and_then(|m| m.first().copied())) {
            return Some(found);
        }
        // node_modules, nearest first.
        let mut at = Some(dir);
        while let Some(d) = at {
            let nm = d.join("node_modules");
            if reads::is_dir(&nm) {
                if let Some(found) = package_types(&nm.join(spec)).or_else(|| package_types(&nm.join("@types").join(types_name(spec)))) {
                    let file = self.file_id(&found);
                    let ambient = self.summary(file).and_then(|summary| summary.ambient_modules.iter().position(|(name, _, _)| &**name == spec));
                    return Some(ambient.map_or(ModRef::File(file), |index| ModRef::Ambient(file, index)));
                }
            }
            at = d.parent();
        }
        None
    }

    /// A module's exports: its summary and the module inside it.
    pub fn module_of(&self, m: ModRef) -> Option<(FileId, Summary, ModuleAt)> {
        match m {
            ModRef::File(f) => {
                let s = self.summary(f)?;
                Some((f, s, ModuleAt::Top))
            }
            ModRef::Ambient(f, i) => {
                let s = self.summary(f)?;
                Some((f, s, ModuleAt::Ambient(i)))
            }
        }
    }

    /// The declarations export `name` of module `m` leads to.
    pub fn export(&self, m: ModRef, name: &str) -> Vec<DeclRef> {
        self.export_hops::<false>(m, name, MAX_HOPS).declarations
    }

    /// Export lookup retaining intermediate bindings, in lookup order.
    pub fn export_resolution(&self, m: ModRef, name: &str) -> ExportResolution {
        self.export_hops::<true>(m, name, MAX_HOPS)
    }

    fn export_hops<const METADATA: bool>(&self, m: ModRef, name: &str, hops: usize) -> ExportResolution {
        if hops == 0 {
            return ExportResolution::default();
        }
        let Some((file, s, at)) = self.module_of(m) else { return ExportResolution::default() };
        let (module, scope) = at.get(&s);
        let refs = |ids: &[u32]| ExportResolution {
            declarations: ids.iter().map(|&decl| DeclRef { file, decl }).collect(),
            aliases: Vec::new(),
        };
        let alias = || ExportAlias {
            file,
            name: name.into(),
            module: match at { ModuleAt::Top => None, ModuleAt::Ambient(i) => Some(s.ambient_modules[i].0.clone()) },
            doc: module.export_docs.get(name).cloned(),
        };
        if name == "=" || (name == "default" && !module.exports.contains_key("default") && module.equals.is_some()) {
            if let Some(eq) = &module.equals {
                return ExportResolution { declarations: self.export_assignment(file, &s, at, eq, hops - 1), aliases: Vec::new() };
            }
        }
        match module.exports.get(name).map(|binding| &binding.target) {
            Some(Export::Decls(ids)) => {
                let mut result = refs(ids);
                if METADATA && module.export_docs.contains_key(name) { result.aliases.push(alias()); }
                result
            }
            Some(Export::Reexport { from, name: inner }) if from.is_empty() => {
                let local = scope.get(inner);
                // A local import re-exported: follow it.
                let mut result = ExportResolution::default();
                if METADATA { result.aliases.push(alias()); }
                for &i in local {
                    let declaration = &s.decls[i as usize];
                    let resolved = match &declaration.kind {
                        super::summary::DeclKind::Import { from, name } => {
                            let mut resolved = self.resolve(file, from).map(|m| self.export_hops::<METADATA>(m, name, hops - 1)).unwrap_or_default();
                            if METADATA {
                                resolved.aliases.insert(0, ExportAlias {
                                    file, name: declaration.name.clone(),
                                    module: declaration.module.map(|i| s.ambient_modules[i].0.clone()),
                                    doc: declaration.doc.clone(),
                                });
                            }
                            resolved
                        }
                        _ => refs(&[i]),
                    };
                    result.extend(resolved);
                }
                result
            }
            Some(Export::Reexport { from, name: inner }) => {
                let mut result = self.resolve(file, from).map(|m| self.export_hops::<METADATA>(m, inner, hops - 1)).unwrap_or_default();
                if METADATA { result.aliases.insert(0, alias()); }
                result
            }
            Some(Export::Namespace(_)) => {
                let mut result = ExportResolution::default();
                if METADATA { result.aliases.push(alias()); }
                result
            }
            None if name == "default" => ExportResolution::default(),
            None => module
                .stars
                .iter()
                .find_map(|from| {
                    let found = self.resolve(file, &from.from).map(|m| self.export_hops::<METADATA>(m, name, hops - 1)).unwrap_or_default();
                    (!found.declarations.is_empty()).then_some(found)
                })
                .unwrap_or_default(),
        }
    }

    fn export_assignment(&self, file: FileId, summary: &Summary, at: ModuleAt, path: &[Box<str>], hops: usize) -> Vec<DeclRef> {
        if hops == 0 { return Vec::new() }
        let Some((root, members)) = path.split_first() else { return Vec::new() };
        let (_, scope) = at.get(summary);
        let local = scope.get(root);
        let local = if local.is_empty() { summary.top.get(root) } else { local };
        let local = if local.is_empty() { summary.globals.get(root) } else { local };
        let mut declarations = local.iter().map(|&decl| DeclRef { file, decl }).collect::<Vec<_>>();
        if declarations.is_empty() {
            declarations = self.globals(file).names.get(root).cloned().unwrap_or_default();
        }
        for member in members {
            let mut next = Vec::new();
            for declaration in declarations {
                let Some(summary) = self.summary(declaration.file) else { continue };
                let declaration_value = &summary.decls[declaration.decl as usize];
                match &declaration_value.kind {
                    super::summary::DeclKind::Namespace => {
                        if let Some(scope) = &declaration_value.scope {
                            next.extend(scope.get(member).iter().map(|&decl| DeclRef { file: declaration.file, decl }));
                        }
                    }
                    super::summary::DeclKind::Import { from, name } if &**name == "*" => {
                        if let Some(module) = self.resolve(declaration.file, from) {
                            next.extend(self.export_hops::<false>(module, member, hops - 1).declarations);
                        }
                    }
                    _ => {}
                }
            }
            declarations = next;
        }
        declarations
    }

    /// The JSDoc of export `name` of `spec` imported from `from_file`:
    /// `None` when that cannot be found, `Some(None)` when it has no doc.
    pub fn export_doc(&self, from_file: &Path, spec: &str, name: &str) -> Option<Option<String>> {
        let from = self.file_id(from_file);
        let m = self.resolve(from, spec)?;
        let decls = self.export(m, name);
        if decls.is_empty() {
            return None;
        }
        let mut doc = None;
        for d in decls {
            if let Some(s) = self.summary(d.file) {
                if let Some(text) = &s.decls[d.decl as usize].doc {
                    doc = Some(text.to_string());
                    break;
                }
            }
        }
        Some(doc)
    }

    // ---- configs and globals -------------------------------------------------

    fn config_for(&self, dir: &Path) -> Option<Arc<Config>> {
        if let Some(c) = self.configs.lock().ok()?.get(dir).cloned() {
            return c.get();
        }
        let cached = Cached::compute(|| {
            let file = dir.join("tsconfig.json");
            if reads::is_file(&file) {
                load_config(&file).map(Arc::new)
            } else {
                dir.parent().and_then(|p| self.config_for(p))
            }
        });
        self.configs.lock().ok()?.insert(dir.to_path_buf(), cached.clone());
        cached.get()
    }

    /// A boolean compiler option of the tsconfig governing `file`: its own
    /// setting, else `strict`'s for the options `strict` implies, else off.
    pub fn strict_option(&self, file: FileId, name: &str) -> bool {
        let flag = |n: &str| self.compiler_flag(file, n);
        flag(name).or_else(|| STRICT_FAMILY.contains(&name).then(|| flag("strict")).flatten()).unwrap_or(false)
    }

    /// A boolean compiler option as the tsconfig governing `file` sets it
    /// (`None` when unset or there is no tsconfig).
    pub fn compiler_flag(&self, file: FileId, name: &str) -> Option<bool> {
        let path = self.path(file);
        path.parent().and_then(|d| self.config_for(d))?.flags.get(name).copied()
    }

    /// Node-format dynamic imports wrap CommonJS values in a namespace.
    pub fn dynamic_import_commonjs(&self, importer: FileId, target: ModRef) -> bool {
        let importer = self.path(importer);
        if !importer.parent().and_then(|dir| self.config_for(dir)).is_some_and(|config| config.node_module == Some(true)) {
            return false;
        }
        let ModRef::File(file) = target else { return false };
        let path = self.path(file);
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("cts") => return true,
            Some("mts") => return false,
            _ => {}
        }
        let mut dir = path.parent();
        while let Some(current) = dir {
            if let Some(text) = reads::read(&current.join("package.json")) {
                if let Ok(package) = serde_json::from_str::<serde_json::Value>(&text) {
                    return package.get("type").and_then(|value| value.as_str()) != Some("module");
                }
                return true;
            }
            dir = current.parent();
        }
        true
    }

    /// The global scope of the tsconfig governing `file`.
    pub fn globals(&self, file: FileId) -> Arc<Globals> {
        let path = self.path(file);
        match path.parent().and_then(|d| self.config_for(d)) {
            Some(c) => self.globals_of(&c),
            None => Arc::new(Globals::default()),
        }
    }

    fn globals_of(&self, config: &Config) -> Arc<Globals> {
        let lib_dir = find_up(&config.dir, &["node_modules", "typescript", "lib"]);
        // The project's own globals come from the config's directory and
        // references, so configs sharing libraries still differ by scope.
        let key = format!("{:?}|{:?}|{:?}|{:?}|{:?}", config.lib, config.types, lib_dir, config.dir, config.references);
        let cell = {
            let mut map = self.globals.lock().expect("globals lock");
            map.entry(key).or_default().clone()
        };
        cell.get_or_init(|| {
            Cached::compute(|| {
                let mut g = self.build_globals(config, lib_dir.as_deref());
                g.id = GLOBALS_BUILT.fetch_add(1, Ordering::Relaxed);
                Arc::new(g)
            })
        })
        .get()
    }

    fn build_globals(&self, config: &Config, lib_dir: Option<&Path>) -> Globals {
        let mut files: Vec<PathBuf> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        // In order: lib files first, as TypeScript loads them, so their
        // overloads come first when declarations merge.
        let mut queue: std::collections::VecDeque<PathBuf> = std::collections::VecDeque::new();
        if let Some(lib_dir) = lib_dir {
            let libs: Vec<String> = if config.lib.is_empty() { vec!["es2023".into(), "dom".into()] } else { config.lib.clone() };
            for l in libs {
                queue.push_back(lib_dir.join(format!("lib.{}.d.ts", canonical_library(&l))));
            }
        }
        let node_modules = find_up(&config.dir, &["node_modules"]);
        if let Some(nm) = &node_modules {
            let mut types: Vec<String> = match &config.types {
                Some(t) => t.clone(),
                None => reads::list(&nm.join("@types")),
            };
            // A program that imports anything built on Node (electron, test
            // runners) has Node's globals even when `types` lists others:
            // packages pull them in with `/// <reference types="node" />`.
            if !types.iter().any(|t| t == "node") && reads::is_dir(&nm.join("@types").join("node")) {
                types.push("node".into());
            }
            for t in types {
                let dir = if t.starts_with('@') && !t.starts_with("@types/") { nm.join(&t) } else { nm.join("@types").join(t.trim_start_matches("@types/")) };
                if let Some(p) = package_types(&dir).or_else(|| package_types(&nm.join(&t))) {
                    queue.push_back(p);
                }
            }
        }
        while let Some(p) = queue.pop_front() {
            let p = normalize_path(&p);
            if !seen.insert(p.clone()) || !reads::is_file(&p) {
                continue;
            }
            let id = self.file_id(&p);
            let Some(s) = self.summary(id) else { continue };
            let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
            for l in &s.ref_libs {
                if let Some(lib_dir) = lib_dir {
                    queue.push_back(lib_dir.join(format!("lib.{}.d.ts", canonical_library(&l.to_lowercase()))));
                }
            }
            for r in &s.ref_paths {
                queue.push_back(dir.join(&**r));
            }
            for t in &s.ref_types {
                if let Some(nm) = &node_modules {
                    if let Some(found) = package_types(&nm.join("@types").join(&**t)).or_else(|| package_types(&nm.join(&**t))) {
                        queue.push_back(found);
                    }
                }
            }
            files.push(p);
        }
        // The project's own globals: `declare global` blocks and script files
        // (no imports or exports) in this project and the ones it references.
        let mut project_files = Vec::new();
        for dir in std::iter::once(&config.dir).chain(&config.references) {
            // Shared setup uses one file's worker allocation. Rule groups may
            // briefly overlap it, but other file workers keep their own budget.
            project_files.extend(reads::global_files(dir, self.discovery_jobs.max(1)));
        }
        project_files.sort_unstable();
        for path in project_files {
            if seen.insert(path.clone()) {
                files.push(path);
            }
        }
        let mut g = Globals::default();
        for p in files {
            let id = self.file_id(&p);
            let Some(s) = self.summary(id) else { continue };
            for (name, ids) in &s.globals.names {
                g.names.entry(name.clone()).or_default().extend(ids.iter().map(|&decl| DeclRef { file: id, decl }));
            }
            for (i, (spec, _, _)) in s.ambient_modules.iter().enumerate() {
                g.modules.entry(spec.clone()).or_default().push(ModRef::Ambient(id, i));
            }
        }
        g
    }
}

/// Which module of a summary: the file itself, or a `declare module`.
#[derive(Clone, Copy, Debug)]
pub enum ModuleAt {
    Top,
    Ambient(usize),
}

impl ModuleAt {
    pub fn get(self, s: &FileSummary) -> (&Module, &Scope) {
        match self {
            ModuleAt::Top => (&s.module, &s.top),
            ModuleAt::Ambient(i) => (&s.ambient_modules[i].1, &s.ambient_modules[i].2),
        }
    }
}

fn summary_of(path: &Path, src: &str) -> Option<Summary> {
    let jsx = super::reads(path).unwrap_or(false);
    let parsed = super::parse(src, jsx).ok()?;
    let dts = [".d.ts", ".d.mts", ".d.cts"].iter().any(|suffix| path.to_string_lossy().ends_with(suffix));
    Some(Arc::new(summarise(&Raw { src, nodes: &parsed.nodes }, parsed.root, dts)))
}

/// `@scope/name` is `@types/scope__name`.
fn types_name(spec: &str) -> String {
    let pkg: String = if spec.starts_with('@') { spec.splitn(3, '/').take(2).collect::<Vec<_>>().join("/") } else { spec.split('/').next().unwrap_or(spec).to_string() };
    let rest = spec.strip_prefix(&pkg).unwrap_or("");
    format!("{}{rest}", pkg.trim_start_matches('@').replace('/', "__"))
}

/// The declaration file of a package (or a file inside it).
fn package_types(dir: &Path) -> Option<PathBuf> {
    if let Some(f) = with_extension(dir) {
        return Some(f);
    }
    let pkg = dir.join("package.json");
    if let Some(text) = reads::read(&pkg) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            for key in ["types", "typings"] {
                if let Some(t) = json.get(key).and_then(|t| t.as_str()) {
                    let p = normalize_path(&dir.join(t));
                    if reads::is_file(&p) {
                        return Some(p);
                    }
                    if let Some(f) = with_extension(&p) {
                        return Some(f);
                    }
                }
            }
        }
    }
    let index = dir.join("index.d.ts");
    reads::is_file(&index).then_some(index)
}

fn find_up(dir: &Path, parts: &[&str]) -> Option<PathBuf> {
    let mut at = Some(dir);
    while let Some(d) = at {
        let mut p = d.to_path_buf();
        for part in parts {
            p.push(part);
        }
        if reads::is_dir(&p) {
            return Some(p);
        }
        at = d.parent();
    }
    None
}

/// `x` as TypeScript would find it: `x.ts`, `x.tsx`, `x.d.ts`, `x/index.ts`...
fn with_extension(base: &Path) -> Option<PathBuf> {
    let s = base.to_string_lossy();
    if [".ts", ".tsx", ".mts", ".cts"].iter().any(|e| s.ends_with(e)) && reads::is_file(base) {
        return Some(base.to_path_buf());
    }
    // `./x.js` in TypeScript source means `./x.ts`.
    let substitutions: &[&str] = if s.ends_with(".mjs") { &[".mts", ".d.mts"] }
        else if s.ends_with(".cjs") { &[".cts", ".d.cts"] } else { &[".ts", ".tsx", ".d.ts"] };
    if let Some(stem) = s.strip_suffix(".js").or_else(|| s.strip_suffix(".mjs")).or_else(|| s.strip_suffix(".cjs")) {
        for ext in substitutions {
            let p = PathBuf::from(format!("{stem}{ext}"));
            if reads::is_file(&p) {
                return Some(p);
            }
        }
    }
    for ext in [".ts", ".tsx", ".d.ts", ".mts", ".cts"] {
        let p = PathBuf::from(format!("{s}{ext}"));
        if reads::is_file(&p) {
            return Some(p);
        }
    }
    for index in ["index.ts", "index.tsx", "index.d.ts"] {
        let p = base.join(index);
        if reads::is_file(&p) {
            return Some(p);
        }
    }
    None
}

/// A tsconfig, following `extends` for `paths`, `baseUrl`, `lib`, `types`.
fn load_config(file: &Path) -> Option<Config> {
    let mut config = Config { dir: file.parent()?.to_path_buf(), ..Config::default() };
    let mut have_paths = false;
    let mut have_lib = false;
    let mut have_types = false;
    let mut current = file.to_path_buf();
    for depth in 0..8 {
        let text = reads::read(&current)?;
        let json: serde_json::Value = serde_json::from_str(&strip_jsonc(&text)).ok()?;
        let dir = current.parent()?.to_path_buf();
        if depth == 0 {
            for r in json.get("references").and_then(|r| r.as_array()).into_iter().flatten() {
                if let Some(p) = r.get("path").and_then(|p| p.as_str()) {
                    let p = normalize_path(&dir.join(p));
                    let d = if p.extension().is_some_and(|e| e == "json") { p.parent().map(Path::to_path_buf).unwrap_or(p) } else { p };
                    config.references.push(d);
                }
            }
        }
        if let Some(o) = json.get("compilerOptions") {
            if config.node_module.is_none() {
                config.node_module = o.get("module").and_then(|value| value.as_str())
                    .map(|mode| matches!(mode.to_ascii_lowercase().as_str(), "node16" | "node18" | "node20" | "nodenext"));
            }
            for (name, value) in o.as_object().into_iter().flatten() {
                if let Some(on) = value.as_bool() {
                    config.flags.entry(name.clone()).or_insert(on);
                }
            }
            if !have_paths {
                if let Some(paths) = o.get("paths").and_then(|p| p.as_object()) {
                    have_paths = true;
                    config.paths = paths
                        .iter()
                        .map(|(k, v)| (k.clone(), v.as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default()))
                        .collect();
                    config.paths_base = o.get("baseUrl").and_then(|b| b.as_str()).map_or(dir.clone(), |b| normalize_path(&dir.join(b)));
                }
            }
            if !have_lib {
                if let Some(lib) = o.get("lib").and_then(|l| l.as_array()) {
                    have_lib = true;
                    config.lib = lib.iter().filter_map(|l| l.as_str().map(str::to_lowercase)).collect();
                }
            }
            if !have_types {
                if let Some(t) = o.get("types").and_then(|l| l.as_array()) {
                    have_types = true;
                    config.types = Some(t.iter().filter_map(|l| l.as_str().map(str::to_string)).collect());
                }
            }
        }
        let Some(parent) = json.get("extends").and_then(|e| e.as_str()) else { break };
        if !parent.starts_with('.') {
            break;
        }
        let mut next = normalize_path(&dir.join(parent));
        if next.extension().is_none() {
            next.set_extension("json");
        }
        current = next;
    }
    Some(config)
}

/// JSON with comments and trailing commas (tsconfig) as plain JSON.
fn strip_jsonc(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(b.len());
                out.push_str(&text[start..i]);
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = text[i + 2..].find("*/").map_or(b.len(), |n| i + n + 4);
            }
            b',' => {
                let rest = text[i + 1..].trim_start();
                if !(rest.starts_with('}') || rest.starts_with(']')) {
                    out.push(',');
                }
                i += 1;
            }
            _ => {
                let ch = text[i..].chars().next().unwrap_or(' ');
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Inputs {
        root: PathBuf,
        buffers: HashSet<PathBuf>,
    }

    impl Inputs {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::current_dir().unwrap().join(format!("target/module-resolution-{}", NEXT.fetch_add(1, Ordering::Relaxed)));
            Self { root, buffers: HashSet::new() }
        }

        fn set(&mut self, name: &str, text: &str) -> PathBuf {
            let path = self.root.join(name);
            reads::set_buffer(&path, text);
            self.buffers.insert(path.clone());
            path
        }

        fn remove(&mut self, path: &Path) {
            reads::drop_buffer(path);
            self.buffers.remove(path);
        }
    }

    impl Drop for Inputs {
        fn drop(&mut self) {
            for path in &self.buffers { reads::drop_buffer(path); }
        }
    }

    fn resolved_path(project: &Project, from: FileId, spec: &str) -> Option<PathBuf> {
        project.resolve(from, spec).map(|module| match module {
            ModRef::File(file) => project.path(file),
            ModRef::Ambient(..) => panic!("file module expected"),
        })
    }

    #[test]
    fn compiler_library_aliases_load_canonical_files_without_wrapper_globals() {
        for (alias, canonical) in [
            ("es6", "es2015"), ("es7", "es2016"),
            ("esnext.asynciterable", "es2018.asynciterable"), ("esnext.symbol", "es2019.symbol"),
            ("esnext.bigint", "es2020.bigint"), ("esnext.weakref", "es2021.weakref"),
            ("esnext.object", "es2024.object"), ("esnext.regexp", "es2024.regexp"),
            ("esnext.string", "es2024.string"), ("esnext.float16", "es2025.float16"),
            ("esnext.iterator", "es2025.iterator"), ("esnext.promise", "es2025.promise"),
        ] {
            for through_reference in [false, true] {
                let mut inputs = Inputs::new();
                let entry = if through_reference { "entry" } else { alias };
                inputs.set("app/tsconfig.json", &format!(r#"{{"compilerOptions":{{"lib":["{entry}"],"types":[]}}}}"#));
                inputs.set("node_modules/typescript/lib/lib.entry.d.ts", &format!("/// <reference lib=\"{alias}\" />"));
                inputs.set(&format!("node_modules/typescript/lib/lib.{alias}.d.ts"), "interface Wrapper {}");
                inputs.set(&format!("node_modules/typescript/lib/lib.{canonical}.d.ts"), "interface Canonical {}");
                let path = inputs.set("app/main.ts", "export {}");
                let project = Project::default();
                let globals = project.globals(project.file_id(&path));
                assert!(globals.names.contains_key("Canonical"), "{alias} (reference {through_reference}) did not load its canonical declarations");
                assert!(!globals.names.contains_key("Wrapper"), "{alias} loaded extra wrapper globals");
            }
        }
    }

    #[test]
    fn export_assignments_preserve_qualified_namespace_targets() {
        use super::super::summary::TypeSyn;
        use super::super::types::{Checker, Env, Ty};

        let mut failures = Vec::new();
        for (name, base, found) in [
            ("simple control", "namespace Outer { export interface Result { x:number } } export=Outer;", true),
            ("qualified", "namespace Outer { export namespace Public { export interface Result { x:number } } } export=Outer.Public;", true),
            ("late declaration", "export=Outer.Public; namespace Outer { export namespace Public { export interface Result { x:number } } }", true),
            ("ambient outer", "declare namespace Outer { namespace Public { interface Result { x:number } } } declare module 'pkg' { export=Outer.Public; }", true),
            ("imported namespace", "import * as Outer from './public'; export=Outer.Public;", true),
            ("missing segment", "namespace Outer { export interface Result { x:number } } export=Outer.Missing;", false),
        ] {
            let mut inputs = Inputs::new();
            inputs.set("tsconfig.json", r#"{"compilerOptions":{"lib":[],"types":[]}}"#);
            inputs.set(if name == "ambient outer" { "base.d.ts" } else { "base.ts" }, base);
            inputs.set("public.ts", "export namespace Public { export interface Result { x:number } }");
            let source = if name == "ambient outer" {
                "import type {Result} from 'pkg';"
            } else {
                "import type {Result} from './base';"
            };
            let path = inputs.set("main.ts", source);
            let project = Project::default();
            let file = project.file_id(&path);
            let checker = Checker::new(&project, file, project.summary(file).unwrap());
            let result = checker.eval(&TypeSyn::Ref(vec!["Result".into()].into(), Vec::new()), &Env::file(file));
            let property = checker.member(&result, "x");
            if if found { checker.equivalent(&property, &Ty::Prim("number")) != Some(true) } else { !property.is_opaque() } {
                failures.push((name, result, property));
            }
        }
        assert!(failures.is_empty(), "qualified exports lost their members: {failures:?}");
    }

    #[test]
    fn package_entries_use_the_matching_ambient_module() {
        use super::super::summary::TypeSyn;
        use super::super::types::{Checker, Env, Ty};

        let mut failures = Vec::new();
        for (name, base, found) in [
            ("self-named", "declare namespace Outer { namespace Public { interface Result { x:number } } } declare module 'pkg' { export=Outer.Public; }", true),
            ("top-level control", "export interface Result { x:number }", true),
            ("other name control", "declare module 'other' { interface Result { x:number } }", false),
        ] {
            let mut inputs = Inputs::new();
            inputs.set("app/tsconfig.json", r#"{"compilerOptions":{"lib":[],"types":[]}}"#);
            inputs.set("node_modules/pkg/package.json", r#"{"types":"index.d.ts"}"#);
            inputs.set("node_modules/pkg/index.d.ts", base);
            let path = inputs.set("app/main.ts", "import type {Result} from 'pkg';");
            let project = Project::default();
            let file = project.file_id(&path);
            let checker = Checker::new(&project, file, project.summary(file).unwrap());
            let result = checker.eval(&TypeSyn::Ref(vec!["Result".into()].into(), Vec::new()), &Env::file(file));
            let property = checker.member(&result, "x");
            if if found { checker.equivalent(&property, &Ty::Prim("number")) != Some(true) } else { !property.is_opaque() } {
                failures.push((name, result, property));
            }
        }
        assert!(failures.is_empty(), "package entry selected the wrong module: {failures:?}");
    }

    #[test]
    fn repeated_resolution_shares_successes_and_misses_without_crossing_importers() {
        let mut inputs = Inputs::new();
        let left = inputs.set("left/item.ts", "export {}");
        let right = inputs.set("right/item.ts", "export {}");
        let other = inputs.set("left/other.ts", "export {}");
        let project = Project::default();
        let from_left = project.file_id(&inputs.root.join("left/main.ts"));
        let from_right = project.file_id(&inputs.root.join("right/main.ts"));
        RESOLUTION_WORK.with(|work| work.set(0));
        for _ in 0..16 {
            assert_eq!(resolved_path(&project, from_left, "./item"), Some(left.clone()));
            assert_eq!(resolved_path(&project, from_right, "./item"), Some(right.clone()));
            assert_eq!(resolved_path(&project, from_left, "./other"), Some(other.clone()));
            assert_eq!(resolved_path(&project, from_left, "./missing"), None);
        }
        let work = RESOLUTION_WORK.with(|work| work.get());
        assert!(work <= 4, "repeated imports probed resolution {work} times");
    }

    #[test]
    fn resolution_hits_replay_reads_for_successes_and_misses() {
        let mut inputs = Inputs::new();
        let item = inputs.set("item.ts", "export {}");
        let project = Project::default();
        let from = project.file_id(&inputs.root.join("main.ts"));
        for (spec, target, present) in [("./item", item, true), ("./missing", inputs.root.join("missing.ts"), false)] {
            let (first, _) = reads::track(|| resolved_path(&project, from, spec));
            let (second, recorded) = reads::track(|| resolved_path(&project, from, spec));
            assert_eq!(first, second);
            assert_eq!(second.is_some(), present);
            assert!(recorded.any(&mut |dep| matches!(dep, Dep::IsFile(path, found) if path == &target && *found == present)));
        }
    }

    #[test]
    fn resolution_invalidation_tracks_creation_deletion_and_extension_precedence() {
        let mut inputs = Inputs::new();
        let fallback = inputs.set("item.tsx", "export {}");
        let project = Project::default();
        let from = project.file_id(&inputs.root.join("main.ts"));
        assert_eq!(resolved_path(&project, from, "./item"), Some(fallback.clone()));
        let preferred = inputs.set("item.ts", "export {}");
        project.invalidate(std::slice::from_ref(&preferred));
        assert_eq!(resolved_path(&project, from, "./item"), Some(preferred.clone()));
        inputs.remove(&preferred);
        project.invalidate(std::slice::from_ref(&preferred));
        assert_eq!(resolved_path(&project, from, "./item"), Some(fallback));
        assert_eq!(resolved_path(&project, from, "./late"), None);
        let late = inputs.set("late.ts", "export {}");
        project.invalidate(std::slice::from_ref(&late));
        assert_eq!(resolved_path(&project, from, "./late"), Some(late.clone()));
        inputs.remove(&late);
        project.invalidate(std::slice::from_ref(&late));
        assert_eq!(resolved_path(&project, from, "./late"), None);
    }

    #[test]
    fn resolution_invalidation_follows_config_and_package_metadata_reads() {
        let mut inputs = Inputs::new();
        let config = inputs.set("tsconfig.json", r#"{"compilerOptions":{"paths":{"alias":["first"],"pkg":["pkg"]},"lib":[],"types":[]}}"#);
        let first = inputs.set("first.ts", "export {}");
        let second = inputs.set("second.ts", "export {}");
        let package = inputs.set("pkg/package.json", r#"{"types":"first.d.ts"}"#);
        let pkg_first = inputs.set("pkg/first.d.ts", "export {}");
        let pkg_second = inputs.set("pkg/second.d.ts", "export {}");
        let project = Project::default();
        let from = project.file_id(&inputs.root.join("main.ts"));
        assert_eq!(resolved_path(&project, from, "alias"), Some(first));
        assert_eq!(resolved_path(&project, from, "pkg"), Some(pkg_first));
        inputs.set("pkg/package.json", r#"{"types":"second.d.ts"}"#);
        project.invalidate(&[package]);
        assert_eq!(resolved_path(&project, from, "pkg"), Some(pkg_second.clone()));
        inputs.set("tsconfig.json", r#"{"compilerOptions":{"paths":{"alias":["second"],"pkg":["pkg"]},"lib":[],"types":[]}}"#);
        project.invalidate(&[config]);
        assert_eq!(resolved_path(&project, from, "alias"), Some(second));
        assert_eq!(resolved_path(&project, from, "pkg"), Some(pkg_second));
    }

    #[test]
    fn jsonc_loses_comments_and_trailing_commas() {
        let v: serde_json::Value = serde_json::from_str(&strip_jsonc("{\n // c\n \"a\": [1, 2,], /* x */ \"b\": \"//\",\n}")).unwrap();
        assert_eq!(v["b"], "//");
        assert_eq!(v["a"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn scoped_packages_map_to_types_names() {
        assert_eq!(types_name("@scope/pkg/sub"), "scope__pkg/sub");
        assert_eq!(types_name("node:fs"), "node:fs");
        assert_eq!(types_name("lodash/fp"), "lodash/fp");
    }
}
