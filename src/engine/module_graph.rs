//! Which of a program's own files import which: built whole on the first
//! question (a module's importers can be any file), then kept in step with
//! edits. A lint reads only its answer ([`Dep::Module`]), so an edit breaks
//! only the files whose answer it changed.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::fx_hash::{FxHashMap, FxHashSet};
use crate::names::modules::resolution::{parse_metadata, program_sources, Configuration, DiscoveryRequest, Host, Json, Origin, Resolver, Settings};
use crate::names::question::{Known, QueryContext, ScopeRef};
use crate::path_globs::PathGlobs;

use super::reads::{self, Dep, DepPath, ModuleAsk, Reads};
use super::units::ParsePlan;
use super::Linter;

type Specifiers = Arc<[Box<str>]>;

/// The graphs of a linter's programs, and what each file's text says.
#[derive(Default)]
pub(super) struct ModuleGraphs {
    graphs: Mutex<FxHashMap<(DepPath, u32), Graph>>,
    summaries: Mutex<FxHashMap<DepPath, Summary>>,
}

/// What a version of a file says of other files; `links` only once asked.
struct Summary {
    hash: u64,
    specifiers: Specifiers,
    links: Option<Arc<Links>>,
}

/// What a file says of modules' names, with specifiers as written.
#[derive(Default, PartialEq)]
pub(super) struct Links {
    /// Specifier and the name read from it.
    pub uses: Vec<(Box<str>, Box<str>)>,
    /// Specifiers whose every name may be read.
    pub whole: Vec<Box<str>>,
    pub published: Vec<Box<str>>,
    /// Published name, specifier, and the name it forwards from there.
    pub forwards: Vec<(Box<str>, Box<str>, Box<str>)>,
    /// Specifiers whose every name it publishes too.
    pub stars: Vec<Box<str>>,
}

/// The name standing for all of a module's names.
const ALL_NAMES: &str = "*";

/// Who reads which of each file's names, derived from [`Links`].
struct NameUses {
    direct: Vec<FxHashMap<Box<str>, Vec<u32>>>,
    whole: Vec<Vec<u32>>,
    /// By forwarded file: the forwarding file, its name, the name forwarded.
    forwards: Vec<Vec<(u32, Box<str>, Box<str>)>>,
    /// By forwarded file: the files publishing all its names, and back.
    starred_by: Vec<Vec<u32>>,
    stars: Vec<Vec<u32>>,
    published: Vec<FxHashSet<Box<str>>>,
    star_excludes: Vec<Box<str>>,
}

impl NameUses {
    fn forwards_star(&self, name: &str) -> bool {
        !self.star_excludes.iter().any(|excluded| &**excluded == name)
    }

    /// Whether `file` has `name` to read, itself or through `export *`.
    fn provides(&self, file: u32, name: &str, seen: &mut Vec<u32>) -> bool {
        if seen.contains(&file) { return false }
        seen.push(file);
        self.published[file as usize].contains(name)
            || self.forwards_star(name) && self.stars[file as usize].iter().any(|&source| self.provides(source, name, seen))
    }

    /// The files reading `file`'s `name`, through whatever forwards it. A
    /// forwarding file nothing imports publishes it to the outside: it counts.
    fn users(&self, importers: &[Vec<u32>], file: u32, name: &str, seen: &mut Vec<(u32, Box<str>)>, users: &mut Vec<u32>) {
        if seen.iter().any(|(at, seen)| *at == file && &**seen == name) { return }
        seen.push((file, name.into()));
        let at = file as usize;
        users.extend_from_slice(&self.whole[at]);
        if let Some(direct) = self.direct[at].get(name) { users.extend_from_slice(direct) }
        let through = |forwarder: u32, published: &str, seen: &mut Vec<(u32, Box<str>)>, users: &mut Vec<u32>| {
            if importers[forwarder as usize].is_empty() { users.push(forwarder) }
            else { self.users(importers, forwarder, published, seen, users) }
        };
        for (forwarder, published, forwarded) in &self.forwards[at] {
            if &**forwarded == name || &**forwarded == ALL_NAMES { through(*forwarder, published, seen, users) }
        }
        if self.forwards_star(name) {
            for &forwarder in &self.starred_by[at] {
                if !self.published[forwarder as usize].contains(name) { through(forwarder, name, seen, users) }
            }
        }
    }
}

struct Graph {
    /// The linter's invalidation this graph is up to date with.
    revision: u64,
    /// What chose the files, and what resolving their specifiers read.
    listing: Arc<Reads>,
    resolving: Arc<Reads>,
    paths: Vec<DepPath>,
    modules: Vec<Box<str>>,
    by_path: FxHashMap<DepPath, u32>,
    by_module: FxHashMap<Box<str>, u32>,
    specifiers: Vec<Specifiers>,
    /// Every file's, once a question about exports asked.
    links: Option<Vec<Arc<Links>>>,
    name_uses: Option<NameUses>,
    /// Answers about exports so far: a rule asks once per export site.
    answers: FxHashMap<ModuleAsk, Arc<ModuleAnswer>>,
    /// By importer's directory and specifier.
    targets: FxHashMap<(PathBuf, Box<str>), Option<u32>>,
    imports: Vec<Vec<u32>>,
    importers: Vec<Vec<u32>>,
    reaches: FxHashMap<(Box<[Box<str>]>, Box<[Box<str>]>), Reaches>,
}

/// Per file, walking its importers upwards without passing `stop` files.
struct Reaches {
    ignored: Vec<bool>,
    stops: Vec<bool>,
    stop: Vec<bool>,
    entry: Vec<bool>,
}

/// What a graph answers for one module.
pub(super) struct ModuleAnswer {
    pub importers: Vec<Box<str>>,
    /// The module is a `stop` file itself.
    pub is_stop: bool,
    /// Nothing imports the module at all, not even `ignore` files.
    pub is_entry: bool,
    /// The module is an `ignore` file itself.
    pub is_ignored: bool,
    pub reaches_stop: bool,
    pub reaches_entry: bool,
    /// By name; only when asked ([`ModuleAsk::exports`]).
    pub exports: Vec<ExportAnswer>,
}

/// What a graph answers for one name a module publishes: `importers` read
/// it, and the walks up from them reach `stop` files or entries.
pub(super) struct ExportAnswer {
    pub name: Box<str>,
    pub importers: Vec<Box<str>>,
    pub reaches_stop: bool,
    pub reaches_entry: bool,
}

impl ModuleAnswer {
    /// The answer's hash, as [`Dep::Module`] records it.
    pub fn hash(answer: Option<&ModuleAnswer>) -> u64 {
        let Some(answer) = answer else { return reads::MISSING };
        let mut bytes = vec![u8::from(answer.is_stop), u8::from(answer.is_entry), u8::from(answer.is_ignored), u8::from(answer.reaches_stop), u8::from(answer.reaches_entry)];
        let strings = |list: &[Box<str>], bytes: &mut Vec<u8>| for text in list {
            bytes.extend_from_slice(text.as_bytes());
            bytes.push(0);
        };
        strings(&answer.importers, &mut bytes);
        for export in &answer.exports {
            bytes.push(1 + u8::from(export.reaches_stop) + 2 * u8::from(export.reaches_entry));
            bytes.extend_from_slice(export.name.as_bytes());
            bytes.push(0);
            strings(&export.importers, &mut bytes);
        }
        reads::hash_bytes(&bytes)
    }
}

impl Graph {
    /// The rows of `module`'s published names, under the walks of `reaches`.
    fn export_answers(&self, module: usize, reaches: &Reaches) -> Vec<ExportAnswer> {
        let Some(uses) = &self.name_uses else { return Vec::new() };
        let mut names: Vec<&Box<str>> = uses.published[module].iter().collect();
        names.sort_unstable();
        names.into_iter().map(|name| {
            let mut users = Vec::new();
            uses.users(&self.importers, module as u32, name, &mut Vec::new(), &mut users);
            users.sort_unstable();
            users.dedup();
            users.retain(|&user| user as usize != module && !reaches.ignored[user as usize]);
            let answered = |test: &dyn Fn(usize) -> bool| users.iter().any(|&user| test(user as usize));
            ExportAnswer {
                name: name.clone(),
                reaches_stop: answered(&|user| reaches.stops[user] || reaches.stop[user]),
                reaches_entry: answered(&|user| !reaches.stops[user] && reaches.entry[user]),
                importers: { let mut importers: Vec<Box<str>> = users.iter().map(|&user| self.modules[user as usize].clone()).collect(); importers.sort_unstable(); importers },
            }
        }).collect()
    }

    fn answer(&mut self, ask: &ModuleAsk) -> Option<Arc<ModuleAnswer>> {
        if let Some(found) = self.answers.get(ask) { return Some(found.clone()) }
        let module = *self.by_module.get(&ask.module)? as usize;
        let key = (ask.stop.clone(), ask.ignore.clone());
        if !self.reaches.contains_key(&key) {
            let reaches = self.reach(&ask.stop, &ask.ignore)?;
            self.reaches.insert(key.clone(), reaches);
        }
        let reaches = self.reaches.get(&key)?;
        let mut importers: Vec<Box<str>> = self.importers[module].iter().map(|&at| at as usize)
            .filter(|&at| !reaches.ignored[at]).map(|at| self.modules[at].clone()).collect();
        importers.sort_unstable();
        let exports = if ask.exports { self.export_answers(module, reaches) } else { Vec::new() };
        let answer = Arc::new(ModuleAnswer { importers, is_stop: reaches.stops[module], is_entry: self.importers[module].is_empty(), is_ignored: reaches.ignored[module], reaches_stop: reaches.stop[module], reaches_entry: reaches.entry[module], exports });
        if ask.exports { self.answers.insert(ask.clone(), answer.clone()); }
        Some(answer)
    }

    /// An importer's walk upwards, run downwards from where walks end: from
    /// every `stop` file, and every entry (a file nothing imports).
    fn reach(&self, stop: &[Box<str>], ignore: &[Box<str>]) -> Option<Reaches> {
        let globs = |list: &[Box<str>]| PathGlobs::new(list.iter().map(|glob| &**glob)).ok();
        let (stop, ignore) = (globs(stop)?, globs(ignore)?);
        let matching = |globs: &PathGlobs| -> Vec<bool> { self.modules.iter().map(|module| globs.matches(Path::new(&**module))).collect() };
        let (stops, ignored) = (matching(&stop), matching(&ignore));
        let spread = |starts: Vec<u32>| {
            let mut reached = vec![false; self.modules.len()];
            let mut pending = starts;
            while let Some(at) = pending.pop() {
                for &target in &self.imports[at as usize] {
                    let index = target as usize;
                    if !std::mem::replace(&mut reached[index], true) && !stops[index] && !ignored[index] { pending.push(target) }
                }
            }
            reached
        };
        let all = || 0..self.modules.len() as u32;
        let stop = spread(all().filter(|&at| stops[at as usize] && !ignored[at as usize]).collect());
        let entries: Vec<u32> = all().filter(|&at| {
            let at = at as usize;
            !stops[at] && !ignored[at] && self.importers[at].iter().all(|&importer| ignored[importer as usize])
        }).collect();
        let mut entry = spread(entries.clone());
        for at in entries { entry[at as usize] = true }
        Some(Reaches { ignored, stops, stop, entry })
    }

    fn link(&mut self) {
        self.importers = vec![Vec::new(); self.modules.len()];
        for (importer, targets) in self.imports.iter().enumerate() {
            for &target in targets { self.importers[target as usize].push(importer as u32) }
        }
        self.reaches.clear();
        self.forget_names();
    }

    fn forget_names(&mut self) {
        self.name_uses = None;
        self.answers.clear();
    }

    /// `specifier`'s target from `file`, resolving what no file asked yet.
    fn target_of(&mut self, resolver: &Resolver<'_, GraphHost<'_>>, file: usize, specifier: &str) -> Option<u32> {
        let directory = self.paths[file].parent()?.to_path_buf();
        let key = (directory, Box::<str>::from(specifier));
        if let Some(&target) = self.targets.get(&key) { return target }
        *resolver.host.program.borrow_mut() = self.paths[file].to_path_buf();
        let found = resolver.resolve_path(&key.0, specifier, &QueryContext::once()).ok().flatten();
        let target = found.and_then(|path| self.by_path.get(&reads::dep_path(&path)).copied());
        self.targets.insert(key, target);
        target
    }

    /// `specifiers`' targets from `file`.
    fn targets_of(&mut self, resolver: &Resolver<'_, GraphHost<'_>>, file: usize, specifiers: &[Box<str>]) -> Vec<u32> {
        let mut targets: Vec<u32> = specifiers.iter().filter_map(|specifier| self.target_of(resolver, file, specifier))
            .filter(|&target| target as usize != file).collect();
        targets.sort_unstable();
        targets.dedup();
        targets
    }

    /// Who reads which name, from every file's [`Links`]. A name the target
    /// doesn't have (`import x = require()` of named exports) reads all of it.
    fn index_names(&mut self, resolver: &Resolver<'_, GraphHost<'_>>) {
        let Some(all) = self.links.clone() else { return };
        let count = all.len();
        let mut uses = NameUses {
            direct: vec![FxHashMap::default(); count],
            whole: vec![Vec::new(); count],
            forwards: vec![Vec::new(); count],
            starred_by: vec![Vec::new(); count],
            stars: vec![Vec::new(); count],
            published: vec![FxHashSet::default(); count],
            star_excludes: resolver.settings.star_excludes.clone(),
        };
        let mut named = Vec::new();
        for (file, links) in all.iter().enumerate() {
            let at = file as u32;
            uses.published[file] = links.published.iter().cloned().chain(links.forwards.iter().map(|(name, ..)| name.clone())).collect();
            for specifier in &links.whole {
                if let Some(target) = self.target_of(resolver, file, specifier).filter(|&target| target != at) { uses.whole[target as usize].push(at) }
            }
            for (name, specifier, forwarded) in &links.forwards {
                if let Some(target) = self.target_of(resolver, file, specifier).filter(|&target| target != at) {
                    uses.forwards[target as usize].push((at, name.clone(), forwarded.clone()));
                }
            }
            for specifier in &links.stars {
                if let Some(target) = self.target_of(resolver, file, specifier).filter(|&target| target != at) {
                    uses.starred_by[target as usize].push(at);
                    uses.stars[file].push(target);
                }
            }
            for (specifier, name) in &links.uses {
                if let Some(target) = self.target_of(resolver, file, specifier).filter(|&target| target != at) { named.push((at, target, name)) }
            }
        }
        for (at, target, name) in named {
            if &**name != ALL_NAMES && uses.provides(target, name, &mut Vec::new()) {
                uses.direct[target as usize].entry(name.clone()).or_default().push(at);
            } else {
                uses.whole[target as usize].push(at);
            }
        }
        self.name_uses = Some(uses);
    }
}

/// A program's files as [`Resolver`] needs them, read straight through
/// [`reads`] (a graph outlives any run).
struct GraphHost<'l> {
    linter: &'l Linter,
    /// A file in the directory whose configuration applies: the root's while
    /// listing the files, each importer's while resolving (its `paths`).
    program: RefCell<PathBuf>,
    configurations: RefCell<FxHashMap<PathBuf, Arc<Configuration>>>,
    metadata: RefCell<FxHashMap<PathBuf, Option<Arc<Json>>>>,
    /// Module resolution probes many candidates per directory.
    listings: RefCell<FxHashMap<PathBuf, Option<Arc<reads::Listing>>>>,
}

impl<'l> GraphHost<'l> {
    fn new(linter: &'l Linter, root: &Path) -> Self {
        GraphHost { linter, program: RefCell::new(root.join("_")), configurations: RefCell::default(), metadata: RefCell::default(), listings: RefCell::default() }
    }

    /// `directory`'s listing, read once ([`reads::list_known`]).
    fn listing(&self, directory: &Path) -> Option<Arc<reads::Listing>> {
        if let Some(listing) = self.listings.borrow().get(directory) { return listing.clone() }
        let listing = reads::list_known(directory, &|parent| self.listing(parent)).map(Arc::new);
        self.listings.borrow_mut().insert(directory.to_path_buf(), listing.clone());
        listing
    }
}

impl Host for GraphHost<'_> {
    fn program(&self) -> Known<u32> { Known::Yes(0) }

    fn path(&self, _: u32) -> Known<PathBuf> { Known::Yes(self.program.borrow().clone()) }

    fn text(&self, path: &Path, _: &QueryContext) -> Known<Option<String>> {
        Known::Yes(reads::read(path).map(|text| text.to_string()))
    }

    fn is_file(&self, path: &Path, _: &QueryContext) -> Known<bool> {
        let found = match path.parent().and_then(|directory| self.listing(directory)) {
            Some(listing) => reads::is_file_listed(path, &listing),
            None => reads::is_file_unrecorded(path),
        };
        reads::note(Dep::IsFile(reads::dep_path(path), found));
        Known::Yes(found)
    }

    fn scope(&self, _: &Path, _: &QueryContext) -> Known<Option<ScopeRef>> { Known::Unknown }

    fn ambient(&self, _: u32, _: &str, _: u64, _: &QueryContext) -> Known<Option<ScopeRef>> { Known::Unknown }

    fn discover(&self, request: &DiscoveryRequest, _: &QueryContext) -> Known<Vec<PathBuf>> {
        self.linter.discovered(request)
    }

    fn metadata(&self, path: &Path, question: &QueryContext) -> Result<Option<Arc<Json>>, ()> {
        if let Some(found) = self.metadata.borrow().get(path) { return Ok(found.clone()) }
        let parsed = parse_metadata(self.text(path, question).known().ok_or(())?.as_deref())?;
        self.metadata.borrow_mut().insert(path.to_path_buf(), parsed.clone());
        Ok(parsed)
    }

    fn configuration(&self, _: &Settings, directory: &Path, load: &mut dyn FnMut() -> Result<Arc<Configuration>, ()>) -> Result<Arc<Configuration>, ()> {
        if let Some(found) = self.configurations.borrow().get(directory) { return Ok(found.clone()) }
        let loaded = load()?;
        self.configurations.borrow_mut().insert(directory.to_path_buf(), loaded.clone());
        Ok(loaded)
    }
}

impl Linter {
    /// What `ask`'s program's graph answers now; `None` when the module is
    /// not one of its files or the program cannot be read.
    pub(super) fn module_answer(&self, ask: &ModuleAsk) -> Option<Arc<ModuleAnswer>> {
        let settings = self.names.settings_at(ask.settings)?;
        let revision = self.invalidation.load(std::sync::atomic::Ordering::Acquire);
        let mut graphs = self.module_graphs.graphs.lock().expect("module graphs");
        let key = (ask.root.clone(), ask.settings);
        let mut graph = match graphs.remove(&key) {
            Some(graph) if graph.revision == revision => graph,
            old => self.fresh_graph(old, settings, &ask.root, revision, ask.exports)?,
        };
        if ask.exports { self.know_names(&mut graph, settings, &ask.root) }
        let answer = graph.answer(ask);
        graphs.insert(key, graph);
        answer
    }

    /// Brings `graph` to where it answers who reads which name.
    fn know_names(&self, graph: &mut Graph, settings: &Settings, root: &Path) {
        if graph.links.is_none() {
            let (_, links) = self.summaries_of_all(settings, &graph.paths, true);
            graph.links = Some(links);
        }
        if graph.name_uses.is_some() { return }
        let host = GraphHost::new(self, root);
        let resolver = Resolver { host: &host, settings };
        let ((), resolved) = reads::track(|| graph.index_names(&resolver));
        graph.resolving = Arc::new(Reads { groups: vec![graph.resolving.clone(), Arc::new(resolved)], ..Default::default() });
    }

    /// The graph at `revision`: `old` brought up to date when the edits
    /// since keep its files and resolutions, else built again.
    fn fresh_graph(&self, old: Option<Graph>, settings: &Settings, root: &Path, revision: u64, links: bool) -> Option<Graph> {
        if let Some(mut graph) = old {
            if let Some((changed, stayed)) = self.invalidated_since(graph.revision) {
                let stale = super::Stale::new(&changed, &stayed);
                if !self.is_stale(&stale, &graph.listing) && !self.is_stale(&stale, &graph.resolving) {
                    self.apply_edits(&mut graph, settings, root, &changed);
                    graph.revision = revision;
                    return Some(graph);
                }
            }
        }
        self.build_graph(settings, root, revision, links)
    }

    /// The files invalidated after `revision` and whether each stayed;
    /// `None` when the log no longer reaches back that far.
    fn invalidated_since(&self, revision: u64) -> Option<(Vec<PathBuf>, Vec<bool>)> {
        let invalidated = self.invalidated.lock().expect("invalidations");
        let (mut changed, mut stayed) = (Vec::new(), Vec::new());
        let newer: Vec<_> = invalidated.iter().filter(|(at, ..)| *at > revision).collect();
        if newer.first().is_some_and(|(at, ..)| *at != revision + 1) { return None }
        for (_, files, kept) in newer {
            changed.extend(files.iter().cloned());
            stayed.extend(kept.iter().copied());
        }
        Some((changed, stayed))
    }

    fn apply_edits(&self, graph: &mut Graph, settings: &Settings, root: &Path, changed: &[PathBuf]) {
        let knows_links = graph.links.is_some();
        let mut edited: Vec<(usize, Specifiers)> = Vec::new();
        for path in changed.iter().collect::<FxHashSet<_>>() {
            let Some(&file) = graph.by_path.get(&reads::dep_path(path)) else { continue };
            let file = file as usize;
            let (specifiers, links) = self.summary_of(settings, &graph.paths[file], knows_links);
            if specifiers != graph.specifiers[file] { edited.push((file, specifiers)) }
            if let (Some(all), Some(links)) = (&mut graph.links, links) {
                if all[file] != links {
                    all[file] = links;
                    graph.forget_names();
                }
            }
        }
        if edited.is_empty() { return }
        let host = GraphHost::new(self, root);
        let resolver = Resolver { host: &host, settings };
        let ((), resolved) = reads::track(|| for (file, specifiers) in &edited {
            graph.imports[*file] = graph.targets_of(&resolver, *file, specifiers);
            graph.specifiers[*file] = specifiers.clone();
        });
        graph.resolving = Arc::new(Reads { groups: vec![graph.resolving.clone(), Arc::new(resolved)], ..Default::default() });
        graph.link();
    }

    fn build_graph(&self, settings: &Settings, root: &Path, revision: u64, links: bool) -> Option<Graph> {
        let host = GraphHost::new(self, root);
        let question = QueryContext::once();
        let (inputs, listing) = reads::track(|| program_sources(settings, &host, &question));
        let Known::Yes(inputs) = inputs else { return None };
        let paths: Vec<DepPath> = inputs.iter()
            .filter(|path| settings.origin(path) == Origin::Project && self.grammar.lang_for(&self.relative(path)).is_some())
            .map(|path| reads::dep_path(path)).collect();
        let modules: Vec<Box<str>> = paths.iter().map(|path| self.relative(path).to_string_lossy().replace('\\', "/").into()).collect();
        let (specifiers, all_links) = self.summaries_of_all(settings, &paths, links);
        let index = |at: usize| u32::try_from(at).expect("files fit u32");
        let mut graph = Graph {
            revision,
            listing: Arc::new(listing),
            resolving: Arc::default(),
            by_path: paths.iter().enumerate().map(|(at, path)| (path.clone(), index(at))).collect(),
            by_module: modules.iter().enumerate().map(|(at, module)| (module.clone(), index(at))).collect(),
            imports: vec![Vec::new(); paths.len()],
            importers: Vec::new(),
            targets: FxHashMap::default(),
            reaches: FxHashMap::default(),
            specifiers,
            links: links.then_some(all_links),
            name_uses: None,
            answers: FxHashMap::default(),
            paths,
            modules,
        };
        let resolver = Resolver { host: &host, settings };
        let ((), resolving) = reads::track(|| for file in 0..graph.paths.len() {
            let specifiers = graph.specifiers[file].clone();
            graph.imports[file] = graph.targets_of(&resolver, file, &specifiers);
        });
        graph.resolving = Arc::new(resolving);
        graph.link();
        Some(graph)
    }

    /// Every file's specifiers and, if asked, links, parsing on several
    /// threads what changed. Links lists are empty when not asked.
    fn summaries_of_all(&self, settings: &Settings, paths: &[DepPath], links: bool) -> (Vec<Specifiers>, Vec<Arc<Links>>) {
        type Found = (Specifiers, Option<Arc<Links>>);
        let workers = self.rule_jobs.clamp(1, paths.len().max(1));
        let mut all: Vec<Found> = vec![(Arc::from([]), None); paths.len()];
        if workers == 1 {
            for (at, path) in paths.iter().enumerate() { all[at] = self.summary_of(settings, path, links) }
        } else {
            let next = std::sync::atomic::AtomicUsize::new(0);
            let family = crate::alloc::family();
            let found: Vec<Vec<(usize, Found)>> = std::thread::scope(|scope| {
                let handles: Vec<_> = (0..workers).map(|_| std::thread::Builder::new().stack_size(super::WORKER_STACK).spawn_scoped(scope, || {
                    let _pool = crate::alloc::own_pool_in(family);
                    let mut found = Vec::new();
                    loop {
                        let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(path) = paths.get(at) else { return found };
                        found.push((at, self.summary_of(settings, path, links)));
                    }
                }).expect("graph worker")).collect();
                handles.into_iter().map(|handle| handle.join().unwrap_or_default()).collect()
            });
            for (at, summary) in found.into_iter().flatten() { all[at] = summary }
        }
        let mut specifiers = Vec::with_capacity(all.len());
        let mut known = Vec::with_capacity(if links { all.len() } else { 0 });
        for (found, links_found) in all {
            specifiers.push(found);
            if links { known.push(links_found.unwrap_or_default()) }
        }
        (specifiers, known)
    }

    /// What `path` says now, parsed only when its text changed or when
    /// `links` are newly asked. `Some` links exactly when asked.
    fn summary_of(&self, settings: &Settings, path: &DepPath, links: bool) -> (Specifiers, Option<Arc<Links>>) {
        let (text, hash) = reads::read_unrecorded(path);
        if let Some(found) = self.module_graphs.summaries.lock().expect("module summaries").get(path) {
            if found.hash == hash && (!links || found.links.is_some()) {
                return (found.specifiers.clone(), found.links.clone().filter(|_| links));
            }
        }
        let (specifiers, found) = text.map_or_else(|| (Arc::from([]), links.then(Arc::default)), |text| self.parse_summary(settings, path, text, hash, links));
        self.module_graphs.summaries.lock().expect("module summaries")
            .insert(path.clone(), Summary { hash, specifiers: specifiers.clone(), links: found.clone() });
        (specifiers, found)
    }

    /// `text` parsed for structure only, and dropped: nothing else keeps the
    /// tree. A reference from a kind declaring no names reads the whole module.
    fn parse_summary(&self, settings: &Settings, path: &Path, text: Arc<str>, hash: u64, links: bool) -> (Specifiers, Option<Arc<Links>>) {
        let nothing = || (Arc::from([]), links.then(Arc::default));
        let rel = self.relative(path);
        let Some(lang) = self.grammar.lang_for(&rel) else { return nothing() };
        let front = self.grammar.native_for(lang);
        let plan = ParsePlan { lang, front, layers: false, overlays: &[], child_layers: false, tokens: &[] };
        let Ok(parsed) = self.parse(path, &text, plan) else { return nothing() };
        let tree = &parsed.tree;
        let kinds: Vec<(u32, Vec<Vec<u16>>)> = settings.references.iter().filter_map(|(kind, fields)| {
            let paths = fields.iter().map(|field| field.split('.').map(|label| self.grammar.labels.get(label)).collect::<Option<Vec<u16>>>()).collect::<Option<Vec<_>>>()?;
            Some((self.grammar.matcher(kind)?, paths))
        }).collect();
        let mut found: Vec<Box<str>> = Vec::new();
        let mut whole: Vec<Box<str>> = Vec::new();
        for (id, node) in tree.nodes.iter().enumerate() {
            let Some((_, paths)) = kinds.iter().find(|(kind, _)| *kind == node.matcher) else { continue };
            let nameless = self.names.kinds.get(node.matcher as usize).is_none_or(|kind| kind.modules.items.is_empty());
            for labels in paths {
                let mut at = Some(id as u32);
                for &label in labels { at = at.and_then(|at| tree.fields(at, label).first().copied()) }
                let Some(end) = at.map(|at| &tree.nodes[at as usize]) else { continue };
                if let Some(specifier) = text.get(end.start as usize..end.end as usize).and_then(quoted) {
                    found.push(specifier.into());
                    if nameless { whole.push(specifier.into()) }
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        if !links { return (found.into(), None) }
        let unit = self.bare_unit(path, lang, text, hash, parsed);
        let (mut said, _) = reads::track(|| self.alone_on(&unit, super::world::module_links));
        said.whole.extend(whole);
        said.whole.sort_unstable();
        said.whole.dedup();
        (found.into(), Some(Arc::new(said)))
    }
}

/// What a quoted literal holds.
fn quoted(text: &str) -> Option<&str> {
    let text = text.trim();
    let quote = text.chars().next().filter(|first| matches!(first, '"' | '\''))?;
    text.strip_prefix(quote)?.strip_suffix(quote).filter(|inner| !inner.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(modules: &[&str], imports: &[&[u32]]) -> Graph {
        let mut graph = Graph {
            revision: 0,
            listing: Arc::default(),
            resolving: Arc::default(),
            paths: modules.iter().map(|module| reads::dep_path(Path::new(module))).collect(),
            modules: modules.iter().map(|&module| module.into()).collect(),
            by_path: FxHashMap::default(),
            by_module: modules.iter().enumerate().map(|(at, &module)| (module.into(), at as u32)).collect(),
            specifiers: Vec::new(),
            links: None,
            name_uses: None,
            answers: FxHashMap::default(),
            targets: FxHashMap::default(),
            imports: imports.iter().map(|targets| targets.to_vec()).collect(),
            importers: Vec::new(),
            reaches: FxHashMap::default(),
        };
        graph.link();
        graph
    }

    fn ask(graph: &mut Graph, module: &str) -> (bool, bool) {
        let ask = ModuleAsk {
            root: reads::dep_path(Path::new("/p")), settings: 0, module: module.into(),
            stop: vec!["old/**".into()].into(), ignore: vec!["test/**".into()].into(), exports: false,
        };
        let answer = graph.answer(&ask).expect("a module of the graph");
        (answer.reaches_stop, answer.reaches_entry)
    }

    #[test]
    fn walks_stop_at_stop_files_and_end_at_entries() {
        let mut graph = graph(
            &["main.ts", "live.ts", "both.ts", "old/up.ts", "chain.ts", "deep.ts", "a.ts", "b.ts", "test/t.ts", "tested.ts", "old/next.ts"],
            &[&[1, 2], &[], &[], &[2, 4, 10], &[5], &[], &[7], &[6], &[9, 4], &[], &[]],
        );
        // An entry, and what only it reaches.
        assert_eq!(ask(&mut graph, "main.ts"), (false, true));
        assert_eq!(ask(&mut graph, "live.ts"), (false, true));
        // Live code and upgrade code both use it.
        assert_eq!(ask(&mut graph, "both.ts"), (true, true));
        // Only upgrade code, directly and through a file only it uses.
        assert_eq!(ask(&mut graph, "chain.ts"), (true, false));
        assert_eq!(ask(&mut graph, "deep.ts"), (true, false));
        // A cycle nothing else imports reaches no entry, and no stop file.
        assert_eq!(ask(&mut graph, "a.ts"), (false, false));
        // Ignored importers don't count: a file only they import is an entry.
        assert_eq!(ask(&mut graph, "tested.ts"), (false, true));
        // A stop file nothing else imports is not an entry.
        assert_eq!(ask(&mut graph, "old/up.ts"), (false, false));
    }

    #[test]
    fn specifiers_are_quoted_literals() {
        assert_eq!(quoted("\"./a\""), Some("./a"));
        assert_eq!(quoted(" './a' "), Some("./a"));
        assert_eq!(quoted("name"), None);
        assert_eq!(quoted("`./a`"), None);
    }
}
