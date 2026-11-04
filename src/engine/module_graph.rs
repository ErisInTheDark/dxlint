//! Which of a program's own files import which: built whole on the first
//! question (a module's importers can be any file), then kept in step with
//! edits. A lint reads only its answer ([`Dep::Module`]), so an edit breaks
//! only the files whose answer it changed.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::fx_hash::{FxHashMap, FxHashSet};
use crate::grammar::names::modules::resolution::{parse_metadata, program_sources, Configuration, DiscoveryRequest, Host, Json, Origin, Resolver, Settings};
use crate::grammar::names::question::{Known, QueryContext, ScopeRef};
use crate::path_globs::PathGlobs;

use super::reads::{self, Dep, DepPath, ModuleAsk, Reads};
use super::units::ParsePlan;
use super::Linter;

type Specifiers = Arc<[Box<str>]>;

/// The graphs of a linter's programs, and every file's specifiers by text.
#[derive(Default)]
pub(super) struct ModuleGraphs {
    graphs: Mutex<FxHashMap<(DepPath, u32), Graph>>,
    specifiers: Mutex<FxHashMap<DepPath, (u64, Specifiers)>>,
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
    pub reaches_stop: bool,
    pub reaches_entry: bool,
}

impl ModuleAnswer {
    /// The answer's hash, as [`Dep::Module`] records it.
    pub fn hash(answer: Option<&ModuleAnswer>) -> u64 {
        let Some(answer) = answer else { return reads::MISSING };
        let mut bytes = vec![u8::from(answer.is_stop), u8::from(answer.reaches_stop), u8::from(answer.reaches_entry)];
        for importer in &answer.importers {
            bytes.extend_from_slice(importer.as_bytes());
            bytes.push(0);
        }
        reads::hash_bytes(&bytes)
    }
}

impl Graph {
    fn answer(&mut self, ask: &ModuleAsk) -> Option<ModuleAnswer> {
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
        Some(ModuleAnswer { importers, is_stop: reaches.stops[module], reaches_stop: reaches.stop[module], reaches_entry: reaches.entry[module] })
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
    }

    /// `specifiers`' targets from `file`, resolving what no file asked yet.
    fn targets_of(&mut self, resolver: &Resolver<'_, GraphHost<'_>>, file: usize, specifiers: &[Box<str>]) -> Vec<u32> {
        let Some(directory) = self.paths[file].parent().map(Path::to_path_buf) else { return Vec::new() };
        *resolver.host.program.borrow_mut() = self.paths[file].to_path_buf();
        let mut targets: Vec<u32> = specifiers.iter().filter_map(|specifier| {
            let key = (directory.clone(), specifier.clone());
            if let Some(&target) = self.targets.get(&key) { return target }
            let found = resolver.resolve_path(&directory, specifier, &QueryContext::once()).ok().flatten();
            let target = found.and_then(|path| self.by_path.get(&*path).copied());
            self.targets.insert(key, target);
            target
        }).filter(|&target| target as usize != file).collect();
        targets.sort_unstable();
        targets.dedup();
        targets
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
}

impl Host for GraphHost<'_> {
    fn program(&self) -> Known<u32> { Known::Yes(0) }

    fn path(&self, _: u32) -> Known<PathBuf> { Known::Yes(self.program.borrow().clone()) }

    fn text(&self, path: &Path, _: &QueryContext) -> Known<Option<String>> {
        Known::Yes(reads::read(path).map(|text| text.to_string()))
    }

    fn is_file(&self, path: &Path, _: &QueryContext) -> Known<bool> {
        let found = match path.parent() {
            Some(directory) => {
                let mut listings = self.listings.borrow_mut();
                let listing = listings.entry(directory.to_path_buf()).or_insert_with(|| reads::list_unrecorded(directory).map(Arc::new));
                listing.as_ref().map_or_else(|| reads::is_file_unrecorded(path), |listing| reads::is_file_listed(path, listing))
            }
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
    pub(super) fn module_answer(&self, ask: &ModuleAsk) -> Option<ModuleAnswer> {
        let settings = self.grammar.names.settings_at(ask.settings)?;
        let revision = self.invalidation.load(std::sync::atomic::Ordering::Acquire);
        let mut graphs = self.module_graphs.graphs.lock().expect("module graphs");
        let key = (ask.root.clone(), ask.settings);
        let mut graph = match graphs.remove(&key) {
            Some(graph) if graph.revision == revision => graph,
            old => self.fresh_graph(old, settings, &ask.root, revision)?,
        };
        let answer = graph.answer(ask);
        graphs.insert(key, graph);
        answer
    }

    /// The graph at `revision`: `old` brought up to date when the edits
    /// since keep its files and resolutions, else built again.
    fn fresh_graph(&self, old: Option<Graph>, settings: &Settings, root: &Path, revision: u64) -> Option<Graph> {
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
        self.build_graph(settings, root, revision)
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
        let edited: Vec<(usize, Specifiers)> = changed.iter().collect::<FxHashSet<_>>().into_iter().filter_map(|path| {
            let file = *graph.by_path.get(&**path)? as usize;
            let specifiers = self.specifiers_of(settings, &graph.paths[file]);
            (specifiers != graph.specifiers[file]).then_some((file, specifiers))
        }).collect();
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

    fn build_graph(&self, settings: &Settings, root: &Path, revision: u64) -> Option<Graph> {
        let host = GraphHost::new(self, root);
        let question = QueryContext::once();
        let (inputs, listing) = reads::track(|| program_sources(settings, &host, &question));
        let Known::Yes(inputs) = inputs else { return None };
        let paths: Vec<DepPath> = inputs.iter()
            .filter(|path| settings.origin(path) == Origin::Project && self.grammar.lang_for(&self.relative(path)).is_some())
            .map(|path| reads::dep_path(path)).collect();
        let modules: Vec<Box<str>> = paths.iter().map(|path| self.relative(path).to_string_lossy().replace('\\', "/").into()).collect();
        let specifiers = self.specifiers_of_all(settings, &paths);
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

    /// Every file's specifiers, parsing on several threads what changed.
    fn specifiers_of_all(&self, settings: &Settings, paths: &[DepPath]) -> Vec<Specifiers> {
        let workers = self.rule_jobs.clamp(1, paths.len().max(1));
        if workers == 1 { return paths.iter().map(|path| self.specifiers_of(settings, path)).collect() }
        let next = std::sync::atomic::AtomicUsize::new(0);
        let family = crate::alloc::family();
        let found: Vec<Vec<(usize, Specifiers)>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers).map(|_| std::thread::Builder::new().stack_size(super::WORKER_STACK).spawn_scoped(scope, || {
                let _pool = crate::alloc::own_pool_in(family);
                let mut found = Vec::new();
                loop {
                    let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(path) = paths.get(at) else { return found };
                    found.push((at, self.specifiers_of(settings, path)));
                }
            }).expect("graph worker")).collect();
            handles.into_iter().map(|handle| handle.join().unwrap_or_default()).collect()
        });
        let mut all: Vec<Specifiers> = vec![Arc::from([]); paths.len()];
        for (at, specifiers) in found.into_iter().flatten() { all[at] = specifiers }
        all
    }

    /// The specifiers `path` holds now, parsed only when its text changed.
    fn specifiers_of(&self, settings: &Settings, path: &DepPath) -> Specifiers {
        let (text, hash) = reads::read_unrecorded(path);
        if let Some((seen, specifiers)) = self.module_graphs.specifiers.lock().expect("module specifiers").get(path) {
            if *seen == hash { return specifiers.clone() }
        }
        let specifiers: Specifiers = text.map_or_else(|| Arc::from([]), |text| self.parse_specifiers(settings, path, &text).into());
        self.module_graphs.specifiers.lock().expect("module specifiers").insert(path.clone(), (hash, specifiers.clone()));
        specifiers
    }

    /// The quoted specifiers `settings.references` finds in `text`, sorted.
    /// Parsed for structure only, and dropped: nothing else keeps the tree.
    fn parse_specifiers(&self, settings: &Settings, path: &Path, text: &str) -> Vec<Box<str>> {
        let rel = self.relative(path);
        let Some(lang) = self.grammar.lang_for(&rel) else { return Vec::new() };
        let front = self.grammar.natives.iter().position(|front| front.frontend.accepts(path));
        let plan = ParsePlan { lang, front, layers: false, overlays: &[], child_layers: false, tokens: &[] };
        let Ok(parsed) = self.parse(path, text, plan) else { return Vec::new() };
        let tree = &parsed.tree;
        let kinds: Vec<(u32, Vec<Vec<u16>>)> = settings.references.iter().filter_map(|(kind, fields)| {
            let paths = fields.iter().map(|field| field.split('.').map(|label| self.grammar.labels.get(label)).collect::<Option<Vec<u16>>>()).collect::<Option<Vec<_>>>()?;
            Some((self.grammar.matcher(kind)?, paths))
        }).collect();
        let mut found: Vec<Box<str>> = Vec::new();
        for (id, node) in tree.nodes.iter().enumerate() {
            let Some((_, paths)) = kinds.iter().find(|(kind, _)| *kind == node.matcher) else { continue };
            for labels in paths {
                let mut at = Some(id as u32);
                for &label in labels { at = at.and_then(|at| tree.fields(at, label).first().copied()) }
                let Some(end) = at.map(|at| &tree.nodes[at as usize]) else { continue };
                if let Some(specifier) = text.get(end.start as usize..end.end as usize).and_then(quoted) { found.push(specifier.into()) }
            }
        }
        found.sort_unstable();
        found.dedup();
        found
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
            stop: vec!["old/**".into()].into(), ignore: vec!["test/**".into()].into(),
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
