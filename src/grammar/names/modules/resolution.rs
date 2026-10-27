use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::super::question::{Known, QueryContext, ScopeRef};
use super::metadata;
pub use super::metadata::Json;
pub use super::configuration::Configuration;
pub use super::inputs::{program_anchor, program_inputs};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DiscoveryRequest {
    pub root: PathBuf,
    pub include: Vec<Box<str>>,
    pub exclude: Vec<Box<str>>,
    pub extensions: Vec<Box<str>>,
}

pub trait Host {
    fn program(&self) -> Known<u32>;
    fn path(&self, file: u32) -> Known<PathBuf>;
    fn text(&self, path: &Path, question: &QueryContext) -> Known<Option<String>>;
    fn is_file(&self, path: &Path, question: &QueryContext) -> Known<bool>;
    fn scope(&self, path: &Path, question: &QueryContext) -> Known<Option<ScopeRef>>;
    fn ambient(&self, importer: u32, name: &str, classes: u64, question: &QueryContext) -> Known<Option<ScopeRef>>;
    fn discover(&self, _: &DiscoveryRequest, _: &QueryContext) -> Known<Vec<PathBuf>> {
        Known::Unknown
    }
    /// `path`'s text as metadata (`None` when there is no such file; an
    /// error when unknown or malformed). Hosts may share parses of one text.
    fn metadata(&self, path: &Path, question: &QueryContext) -> Result<Option<Arc<Json>>, ()> {
        parse_metadata(self.text(path, question).known().ok_or(())?.as_deref())
    }
    /// The configuration `settings` loads for the program in `directory`,
    /// as `load` builds it. Hosts may share it between resolutions.
    fn configuration(&self, _settings: &Settings, _directory: &Path, load: &mut dyn FnMut() -> Result<Arc<Configuration>, ()>) -> Result<Arc<Configuration>, ()> {
        load()
    }
    /// What package directory `root` offers for `subpath` under
    /// `conditions`, as `probe` finds it. Every importer below `root` asks
    /// the same: hosts may share it (with its reads) between resolutions.
    fn package(&self, _settings: &Settings, _root: &Path, _subpath: &str, _conditions: &[&str], probe: &mut dyn FnMut() -> Result<Probe<PathBuf>, ()>) -> Result<Probe<PathBuf>, ()> {
        probe()
    }
}

pub fn parse_metadata(text: Option<&str>) -> Result<Option<Arc<Json>>, ()> {
    text.map(|text| metadata::parse(text).map(Arc::new).ok_or(())).transpose()
}

#[derive(Clone, Copy, Debug)]
pub enum Search {
    Relative,
    Paths,
    Base,
    Ambient,
    Packages,
}

#[derive(Clone, Debug, Default)]
pub struct Config {
    pub files: Vec<Box<str>>,
    pub source_files: Box<str>,
    pub include: Box<str>,
    pub exclude: Box<str>,
    pub extends: Box<str>,
    pub references: Box<str>,
    pub options: Box<str>,
    pub paths: Box<str>,
    pub base: Box<str>,
}

#[derive(Clone, Debug, Default)]
pub struct Packages {
    pub directories: Vec<Box<str>>,
    pub metadata: Box<str>,
    pub entries: Vec<Box<str>>,
    pub exports: Box<str>,
    pub imports: Box<str>,
    pub scoped_prefix: Box<str>,
    pub type_directory: Box<str>,
    pub type_join: Box<str>,
}

#[derive(Clone, Debug, Default)]
pub struct Globals {
    /// Where a default library file lies, first found per package directory
    /// (one TypeScript release ships them in `typescript`, the next in a
    /// platform package).
    pub libraries: Vec<Box<str>>,
    pub library_option: Box<str>,
    pub default_libraries: Vec<Box<str>>,
    pub types_option: Box<str>,
    pub type_roots_option: Box<str>,
    pub type_directories: Vec<Box<str>>,
    pub project_files: Vec<Box<str>>,
    pub references: bool,
    /// When set: a program file can publish a global name only if its text
    /// contains that name or one of these (escapes, re-export syntax...),
    /// so other files are never parsed for that name.
    pub name_hints: Vec<Box<str>>,
    /// When set: a program file can declare an ambient module scope only if
    /// its text contains one of these.
    pub ambient_hints: Vec<Box<str>>,
}

impl Globals {
    /// Whether a file with `text` may publish global `name`.
    pub fn may_publish(&self, text: &str, name: &str) -> bool {
        self.name_hints.is_empty() || text.contains(name) || self.name_hints.iter().any(|hint| text.contains(&**hint))
    }

    /// Whether a file with `text` may declare an ambient module scope.
    pub fn may_declare_ambient(&self, text: &str) -> bool {
        self.ambient_hints.is_empty() || self.ambient_hints.iter().any(|hint| text.contains(&**hint))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Settings {
    pub search: Vec<Search>,
    pub ambient_scopes: u64,
    pub extensions: Vec<Box<str>>,
    pub substitutions: Vec<(Box<str>, Vec<Box<str>>)>,
    pub candidates: Vec<Box<str>>,
    pub config: Config,
    pub packages: Packages,
    pub globals: Globals,
    pub conditions: Vec<Box<str>>,
    pub star_excludes: Vec<Box<str>>,
}

pub struct Resolver<'a, H: ?Sized> {
    pub host: &'a H,
    pub settings: &'a Settings,
}

impl<H: Host + ?Sized> Resolver<'_, H> {
    pub(super) fn program_directory(&self) -> Result<PathBuf, ()> {
        let program = self.host.program().known().ok_or(())?;
        let path = self.host.path(program).known().ok_or(())?;
        path.parent().map(Path::to_path_buf).ok_or(())
    }

    /// The program's configuration, through the host's cache.
    pub(super) fn configuration(&self, question: &QueryContext) -> Result<Arc<Configuration>, ()> {
        let directory = self.program_directory()?;
        self.host.configuration(self.settings, &directory, &mut || Configuration::load(self, &directory, question).map(Arc::new))
    }

    pub fn context(&self, importer: u32, question: &QueryContext) -> Known<crate::expr::Value<'static>> {
        super::context::context(self, importer, question)
    }

    pub fn resolve(&self, importer: u32, specifier: &str, conditions: &[&str], question: &QueryContext) -> Known<Option<ScopeRef>> {
        match self.resolve_inner(importer, specifier, conditions, question) {
            Ok(scope) => Known::Yes(scope),
            Err(()) => Known::Unknown,
        }
    }

    fn resolve_inner(&self, importer: u32, specifier: &str, conditions: &[&str], question: &QueryContext) -> Result<Option<ScopeRef>, ()> {
        let path = self.host.path(importer).known().ok_or(())?;
        let dir = path.parent().ok_or(())?;
        let relative = Path::new(specifier).is_absolute() || specifier.starts_with("./") || specifier.starts_with("../");
        let mut config = None;
        for search in &self.settings.search {
            spend(question)?;
            let result = match search {
                Search::Relative if relative => self.candidates(dir, specifier, question)?,
                Search::Paths | Search::Base if !relative => {
                    if config.is_none() { config = Some(self.configuration(question)?); }
                    let config = config.as_ref().ok_or(())?;
                    let base = config.base(&self.settings.config.base)?;
                    match search {
                        Search::Base => match base { Some(base) => self.candidates(&base, specifier, question)?, None => None },
                        _ => self.paths(config, base.as_deref(), specifier, question)?,
                    }
                }
                Search::Ambient if !relative => self.host.ambient(importer, specifier, self.settings.ambient_scopes, question).known().ok_or(())?,
                Search::Packages if !relative => match self.packages(dir, specifier, conditions, question)? {
                    Probe::Found(scope) => Some(scope),
                    Probe::Blocked => return Ok(None),
                    Probe::Missing => None,
                },
                _ => None,
            };
            if result.is_some() { return Ok(result) }
        }
        Ok(None)
    }

    fn candidates(&self, dir: &Path, name: &str, question: &QueryContext) -> Result<Option<ScopeRef>, ()> {
        self.candidate_path(dir, name, question)?.map(|path| self.host.scope(&path, question).known().ok_or(())).transpose().map(Option::flatten)
    }

    fn candidate_path(&self, dir: &Path, name: &str, question: &QueryContext) -> Result<Option<PathBuf>, ()> {
        let substitution = self.settings.substitutions.iter().find(|(suffix, _)| name.ends_with(&**suffix));
        let (base, mut extensions): (&str, Vec<&str>) = match substitution {
            Some((suffix, replacements)) => (&name[..name.len() - suffix.len()], replacements.iter().map(|value| &**value).chain(std::iter::once(&**suffix)).collect()),
            None => (name, std::iter::once("").chain(self.settings.extensions.iter().map(|value| &**value)).collect()),
        };
        extensions.dedup();
        for template in &self.settings.candidates {
            for extension in &extensions {
                spend(question)?;
                let candidate = expand(template, &[("dir", &dir.to_string_lossy()), ("name", base), ("ext", extension)])?;
                let candidate = clean(Path::new(&candidate));
                if self.host.is_file(&candidate, question).known().ok_or(())? {
                    return Ok(Some(candidate));
                }
            }
        }
        Ok(None)
    }

    pub(super) fn metadata(&self, path: &Path, question: &QueryContext) -> Result<Option<Arc<Json>>, ()> {
        spend(question)?;
        self.host.metadata(path, question)
    }

    fn paths(&self, config: &Configuration, base: Option<&Path>, specifier: &str, question: &QueryContext) -> Result<Option<ScopeRef>, ()> {
        let Some(paths) = config.options.get(&*self.settings.config.paths) else { return Ok(None) };
        let Some((targets, capture)) = mapped(&paths.value, specifier) else { return Ok(None) };
        let base = base.unwrap_or(&paths.directory);
        for target in targets.array().ok_or(())? {
            let target = target.text().ok_or(())?.replace('*', capture);
            if let Some(scope) = self.candidates(base, &target, question)? { return Ok(Some(scope)) }
        }
        Ok(None)
    }

    fn packages(&self, dir: &Path, specifier: &str, conditions: &[&str], question: &QueryContext) -> Result<Probe, ()> {
        let parts = specifier.split('/').collect::<Vec<_>>();
        if parts.is_empty() || parts.iter().any(|part| part.is_empty() || *part == "." || *part == "..") { return Ok(Probe::Blocked) }
        let count = if !self.settings.packages.scoped_prefix.is_empty() && specifier.starts_with(&*self.settings.packages.scoped_prefix) { 2 } else { 1 };
        if parts.len() < count { return Ok(Probe::Blocked) }
        let package = parts[..count].join("/");
        let subpath = if parts.len() == count { ".".to_owned() } else { format!("./{}", parts[count..].join("/")) };
        for ancestor in dir.ancestors() {
            spend(question)?;
            for directory in &self.settings.packages.directories {
                let root = ancestor.join(&**directory).join(&package);
                match self.package(&root, &subpath, conditions, question)? {
                    Probe::Missing => {}
                    result => return Ok(result),
                }
            }
        }
        Ok(Probe::Missing)
    }

    fn package(&self, root: &Path, subpath: &str, conditions: &[&str], question: &QueryContext) -> Result<Probe, ()> {
        let found = self.host.package(self.settings, root, subpath, conditions, &mut || self.package_path(root, subpath, conditions, question))?;
        Ok(match found {
            Probe::Found(path) => self.host.scope(&path, question).known().ok_or(())?.map_or(Probe::Missing, Probe::Found),
            Probe::Blocked => Probe::Blocked,
            Probe::Missing => Probe::Missing,
        })
    }

    pub(super) fn package_path(&self, root: &Path, subpath: &str, conditions: &[&str], question: &QueryContext) -> Result<Probe<PathBuf>, ()> {
        if let Some(metadata) = self.metadata(&root.join(&*self.settings.packages.metadata), question)? {
            if let Some(exports) = metadata.get(&self.settings.packages.exports) {
                let choice = match exports {
                    Json::Object(entries) if entries.iter().any(|(key, _)| key.starts_with('.')) => {
                        mapped(exports, subpath).map(|(value, capture)| (value, capture.to_owned()))
                    }
                    _ if subpath == "." => Some((exports, String::new())),
                    _ => None,
                };
                let Some((choice, capture)) = choice else { return Ok(Probe::Blocked) };
                let targets = self.export_targets(choice, conditions, &capture, question)?;
                for target in targets {
                    let path = Path::new(&target);
                    if path.is_absolute() || path.components().any(|part| part == std::path::Component::ParentDir) { continue }
                    if let Some(path) = self.candidate_path(root, &target, question)? { return Ok(Probe::Found(path)) }
                }
                return Ok(Probe::Blocked);
            }
            if subpath == "." {
                for entry in &self.settings.packages.entries {
                    if let Some(target) = metadata.get(entry).and_then(Json::text) {
                        if let Some(path) = self.candidate_path(root, target, question)? { return Ok(Probe::Found(path)) }
                    }
                }
            }
        }
        let name = subpath.strip_prefix("./").unwrap_or("");
        Ok(self.candidate_path(root, name, question)?.map_or(Probe::Missing, Probe::Found))
    }

    fn export_targets(&self, value: &Json, conditions: &[&str], capture: &str, question: &QueryContext) -> Result<Vec<String>, ()> {
        spend(question)?;
        match value {
            Json::Text(target) => Ok(vec![target.replace('*', capture)]),
            Json::Array(values) => {
                let mut targets = Vec::new();
                for value in values { targets.extend(self.export_targets(value, conditions, capture, question)?); }
                Ok(targets)
            }
            Json::Object(entries) => {
                for (condition, target) in entries {
                    if conditions.contains(&condition.as_str()) || self.settings.conditions.iter().any(|name| &**name == condition) {
                        let result = self.export_targets(target, conditions, capture, question)?;
                        if !result.is_empty() || matches!(target, Json::Null) { return Ok(result) }
                    }
                }
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        }
    }
}

#[derive(Clone)]
pub enum Probe<T = ScopeRef> {
    Missing,
    Blocked,
    Found(T),
}

pub(super) fn spend(question: &QueryContext) -> Result<(), ()> {
    question.budget.step().then_some(()).ok_or(())
}

pub(super) fn clean(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => { result.pop(); }
            other => result.push(other),
        }
    }
    result
}

/// Where a program file comes from, by its path alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// A default library file (the `libraries` template, any name).
    Library,
    /// A file inside an installed package; the package name.
    Package(Box<str>),
    Project,
}

impl Settings {
    pub fn origin(&self, path: &Path) -> Origin {
        let text = clean(path).to_string_lossy().replace('\\', "/");
        if self.is_library(&text) { return Origin::Library }
        let parts: Vec<&str> = text.split('/').collect();
        let directory = parts.iter().rposition(|part| self.packages.directories.iter().any(|name| **name == **part));
        if let Some(rest) = directory.map(|at| &parts[at + 1..]) {
            let prefix = &*self.packages.scoped_prefix;
            let count = if !prefix.is_empty() && rest.first().is_some_and(|part| part.starts_with(prefix)) { 2 } else { 1 };
            if rest.len() > count { return Origin::Package(rest[..count].join("/").into()) }
        }
        Origin::Project
    }

    /// Whether `path` fits a `libraries` template.
    fn is_library(&self, path: &str) -> bool {
        self.globals.libraries.iter().any(|template| self.fits_library(template, path))
    }

    /// Whether `path` fits `template`: `{package_dir}` is any prefix ending
    /// in a package directory, other placeholders one segment.
    fn fits_library(&self, template: &str, path: &str) -> bool {
        let mut reached = vec![false; path.len() + 1];
        reached[0] = true;
        let mut rest = template;
        if rest.is_empty() { return false }
        while !rest.is_empty() {
            let piece = match rest.find('{') {
                Some(0) => {
                    let Some(close) = rest.find('}') else { return false };
                    let (name, _) = TemplateTransform::parse(&rest[1..close]).unwrap_or(("", TemplateTransform::Identity));
                    rest = &rest[close + 1..];
                    if name == "package_dir" { Piece::PackageDir } else { Piece::Segment }
                }
                found => {
                    let end = found.unwrap_or(rest.len());
                    let literal = &rest[..end];
                    rest = &rest[end..];
                    Piece::Literal(literal)
                }
            };
            let mut next = vec![false; reached.len()];
            for from in (0..reached.len()).filter(|&at| reached[at]) {
                for to in from..reached.len() {
                    if next[to] || !path.is_char_boundary(to) { continue }
                    next[to] = match piece {
                        Piece::Literal(literal) => &path[from..to] == literal,
                        Piece::Segment => to > from && !path[from..to].contains('/'),
                        Piece::PackageDir => {
                            let last = path[..to].rsplit('/').next().unwrap_or("");
                            self.packages.directories.iter().any(|name| **name == *last)
                        }
                    };
                }
            }
            if !next.contains(&true) { return false }
            reached = next;
        }
        reached[path.len()]
    }
}

enum Piece<'a> {
    Literal(&'a str),
    Segment,
    PackageDir,
}

pub(super) enum TemplateTransform {
    Identity,
    Lower,
}

impl TemplateTransform {
    pub(super) fn parse(placeholder: &str) -> Result<(&str, Self), ()> {
        match placeholder.split_once(':') {
            None => Ok((placeholder, Self::Identity)),
            Some((name, "lower")) => Ok((name, Self::Lower)),
            Some(_) => Err(()),
        }
    }
}

pub(super) fn expand(template: &str, values: &[(&str, &str)]) -> Result<String, ()> {
    let mut output = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        output.push_str(&rest[..open]);
        let close = rest[open + 1..].find('}').ok_or(())? + open + 1;
        let (key, transform) = TemplateTransform::parse(&rest[open + 1..close])?;
        let value = values.iter().find(|(name, _)| *name == key).ok_or(())?.1;
        match transform {
            TemplateTransform::Identity => output.push_str(value),
            TemplateTransform::Lower => output.push_str(&value.to_lowercase()),
        }
        rest = &rest[close + 1..];
    }
    if rest.contains('}') { return Err(()) }
    output.push_str(rest);
    Ok(output)
}

fn mapped<'a>(value: &'a Json, name: &'a str) -> Option<(&'a Json, &'a str)> {
    let Json::Object(entries) = value else { return None };
    if let Some((_, value)) = entries.iter().find(|(key, _)| key == name) { return Some((value, "")) }
    entries.iter().filter_map(|(key, value)| {
        let (prefix, suffix) = key.split_once('*')?;
        let middle = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
        Some((prefix.len(), suffix.len(), value, middle))
    }).max_by_key(|(prefix, suffix, _, _)| (*prefix, *suffix)).map(|(_, _, value, middle)| (value, middle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Files {
        paths: BTreeMap<PathBuf, (u32, String)>,
    }

    impl Files {
        fn add(&mut self, path: &str, text: &str) -> ScopeRef {
            let file = self.paths.len() as u32;
            self.paths.insert(path.into(), (file, text.into()));
            ScopeRef { file, scope: 0 }
        }
    }

    impl Host for Files {
        fn program(&self) -> Known<u32> { Known::Yes(0) }
        fn path(&self, file: u32) -> Known<PathBuf> {
            self.paths.iter().find_map(|(path, (id, _))| (*id == file).then(|| Known::Yes(path.clone()))).unwrap_or(Known::Unknown)
        }
        fn text(&self, path: &Path, _: &QueryContext) -> Known<Option<String>> {
            Known::Yes(self.paths.get(path).map(|(_, text)| text.clone()))
        }
        fn is_file(&self, path: &Path, _: &QueryContext) -> Known<bool> {
            Known::Yes(self.paths.contains_key(path))
        }
        fn scope(&self, path: &Path, _: &QueryContext) -> Known<Option<ScopeRef>> {
            Known::Yes(self.paths.get(path).map(|(file, _)| ScopeRef { file: *file, scope: 0 }))
        }
        fn ambient(&self, _: u32, _: &str, _: u64, _: &QueryContext) -> Known<Option<ScopeRef>> {
            Known::Yes(None)
        }
    }

    fn settings() -> Settings {
        Settings {
            search: vec![Search::Relative, Search::Packages],
            extensions: vec![".ts".into(), ".d.ts".into()],
            substitutions: vec![(".js".into(), vec![".ts".into(), ".d.ts".into()])],
            candidates: vec!["{dir}/{name}{ext}".into(), "{dir}/{name}/index{ext}".into()],
            packages: Packages {
                directories: vec!["node_modules".into()],
                metadata: "package.json".into(),
                entries: vec!["types".into()],
                exports: "exports".into(),
                imports: "imports".into(),
                scoped_prefix: "@".into(),
                type_directory: "@types".into(),
                type_join: "__".into(),
            },
            ..Settings::default()
        }
    }

    #[test]
    fn origins_tell_libraries_from_packages_and_project_files() {
        let mut settings = settings();
        settings.globals.libraries = vec!["{package_dir}/typescript/lib/lib.{name:lower}.d.ts".into(), "{package_dir}/@typescript/typescript-linux-x64/lib/lib.{name:lower}.d.ts".into()];
        let origin = |path: &str| settings.origin(Path::new(path));
        assert_eq!(origin("/app/node_modules/typescript/lib/lib.es2015.promise.d.ts"), Origin::Library);
        assert_eq!(origin("/app/node_modules/@typescript/typescript-linux-x64/lib/lib.dom.d.ts"), Origin::Library);
        assert_eq!(origin("/app/node_modules/typescript/lib/typescript.d.ts"), Origin::Package("typescript".into()));
        assert_eq!(origin("/app/node_modules/.pnpm/x@1/node_modules/@types/node/globals.d.ts"), Origin::Package("@types/node".into()));
        assert_eq!(origin("/app/lib/lib.dom.d.ts"), Origin::Project);
        assert_eq!(origin("/app/src/promise.ts"), Origin::Project);
    }

    #[test]
    fn ordered_substitution_candidates_choose_the_first_existing_file() {
        let mut files = Files::default();
        let importer = files.add("/app/main.ts", "");
        let preferred = files.add("/app/leaf.ts", "");
        files.add("/app/leaf.d.ts", "");
        let settings = settings();
        assert_eq!(Resolver { host: &files, settings: &settings }.resolve(importer.file, "./leaf.js", &[], &QueryContext::default()), Known::Yes(Some(preferred)));
    }

    #[test]
    fn package_conditions_keep_metadata_order_instead_of_sorting_keys() {
        let mut files = Files::default();
        let importer = files.add("/app/main.ts", "");
        files.add("/app/node_modules/pkg/package.json", r#"{"exports":{".":{"z-custom":"./custom.d.ts","a-custom":"./other.d.ts"}}}"#);
        let preferred = files.add("/app/node_modules/pkg/custom.d.ts", "");
        files.add("/app/node_modules/pkg/other.d.ts", "");
        let settings = settings();
        assert_eq!(Resolver { host: &files, settings: &settings }.resolve(importer.file, "pkg", &["z-custom", "a-custom"], &QueryContext::default()), Known::Yes(Some(preferred)));
    }

    #[test]
    fn an_explicit_null_export_does_not_fall_back_to_package_types() {
        let mut files = Files::default();
        let importer = files.add("/app/main.ts", "");
        files.add("/app/node_modules/pkg/package.json", r#"{"exports":{".":null},"types":"fallback.d.ts"}"#);
        files.add("/app/node_modules/pkg/fallback.d.ts", "");
        let settings = settings();
        assert_eq!(Resolver { host: &files, settings: &settings }.resolve(importer.file, "pkg", &[], &QueryContext::default()), Known::Yes(None));
    }

    #[test]
    fn duplicate_conditions_replace_values_without_changing_priority() {
        let mut files = Files::default();
        let importer = files.add("/app/main.ts", "");
        files.add("/app/node_modules/pkg/package.json", r#"{"exports":{".":{"z-custom":"./old.d.ts","a-custom":"./other.d.ts","z-custom":"./current.d.ts"}}}"#);
        files.add("/app/node_modules/pkg/old.d.ts", "");
        files.add("/app/node_modules/pkg/other.d.ts", "");
        let preferred = files.add("/app/node_modules/pkg/current.d.ts", "");
        let settings = settings();
        assert_eq!(Resolver { host: &files, settings: &settings }.resolve(importer.file, "pkg", &["z-custom", "a-custom"], &QueryContext::default()), Known::Yes(Some(preferred)));
    }

    fn configured() -> Settings {
        let mut settings = settings();
        settings.search = vec![Search::Relative, Search::Paths, Search::Base, Search::Packages];
        settings.config = Config {
            files: vec!["tsconfig.json".into()],
            extends: "extends".into(),
            options: "compilerOptions".into(),
            paths: "paths".into(),
            base: "baseUrl".into(),
            ..Config::default()
        };
        settings
    }

    #[test]
    fn inherited_base_is_relative_to_the_file_that_declared_it() {
        let mut files = Files::default();
        let importer = files.add("/app/main.ts", "");
        files.add("/app/tsconfig.json", r#"{"extends":"../base/tsconfig.json"}"#);
        files.add("/base/tsconfig.json", "{/* base */\"compilerOptions\":{\"baseUrl\":\"./src\",},}");
        let preferred = files.add("/base/src/leaf.ts", "");
        files.add("/app/src/leaf.ts", "");
        let settings = configured();
        assert_eq!(Resolver { host: &files, settings: &settings }.resolve(importer.file, "leaf", &[], &QueryContext::default()), Known::Yes(Some(preferred)));
    }

    #[test]
    fn overriding_base_relocates_inherited_path_targets_but_not_the_base_option() {
        let mut files = Files::default();
        let importer = files.add("/app/main.ts", "");
        files.add("/app/tsconfig.json", r#"{"extends":"../base/tsconfig.json","compilerOptions":{"baseUrl":"./child"}}"#);
        files.add("/base/tsconfig.json", r#"{"compilerOptions":{"baseUrl":"./src","paths":{"x":["leaf"]}}}"#);
        let preferred = files.add("/app/child/leaf.ts", "");
        files.add("/base/src/leaf.ts", "");
        let settings = configured();
        assert_eq!(Resolver { host: &files, settings: &settings }.resolve(importer.file, "x", &[], &QueryContext::default()), Known::Yes(Some(preferred)));
    }

    #[test]
    fn foreign_imports_keep_program_config_but_resolve_relative_to_their_source() {
        let mut files = Files::default();
        files.add("/app/main.ts", "");
        files.add("/app/tsconfig.json", r#"{"compilerOptions":{"baseUrl":"./src","paths":{"alias":["leaf"]}}}"#);
        let target = files.add("/app/src/leaf.ts", "");
        let importer = files.add("/foreign/main.ts", "");
        files.add("/foreign/tsconfig.json", r#"{"compilerOptions":{"baseUrl":"./wrong","paths":{"alias":["leaf"]}}}"#);
        files.add("/foreign/wrong/leaf.ts", "");
        let relative = files.add("/foreign/leaf.ts", "");
        let package = files.add("/foreign/node_modules/pkg/index.ts", "");
        let settings = configured();
        let resolver = Resolver { host: &files, settings: &settings };
        let question = QueryContext::default();
        assert_eq!(resolver.resolve(importer.file, "./leaf", &[], &question), Known::Yes(Some(relative)));
        assert_eq!(resolver.resolve(importer.file, "pkg", &[], &question), Known::Yes(Some(package)));
        assert_eq!(resolver.resolve(importer.file, "alias", &[], &question), Known::Yes(Some(target)));
        assert_eq!(resolver.resolve(importer.file, "leaf", &[], &question), Known::Yes(Some(target)));
    }
}
