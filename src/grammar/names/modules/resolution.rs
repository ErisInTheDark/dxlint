use std::path::{Path, PathBuf};

use super::super::question::{Known, QueryContext, ScopeRef};
use super::metadata::{self, Json};
use super::configuration::Configuration;
pub use super::inputs::program_inputs;

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
    pub libraries: Box<str>,
    pub library_option: Box<str>,
    pub default_libraries: Vec<Box<str>>,
    pub types_option: Box<str>,
    pub type_roots_option: Box<str>,
    pub type_directories: Vec<Box<str>>,
    pub project_files: Vec<Box<str>>,
    pub references: bool,
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
                    if config.is_none() { config = Some(Configuration::load(self, &self.program_directory()?, question)?); }
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

    pub(super) fn metadata(&self, path: &Path, question: &QueryContext) -> Result<Option<Json>, ()> {
        spend(question)?;
        self.host.text(path, question).known().ok_or(())?.map(|text| metadata::parse(&text).ok_or(())).transpose()
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
        Ok(match self.package_path(root, subpath, conditions, question)? {
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

pub(super) enum Probe<T = ScopeRef> {
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
