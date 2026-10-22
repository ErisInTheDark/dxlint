use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::configuration::{Configuration, OptionValue};
use super::metadata::Json;
use super::resolution::{clean, expand, spend, DiscoveryRequest, Host, Probe, Resolver, Settings};
use super::super::question::{Known, QueryContext};

pub fn program_inputs(settings: &Settings, host: &dyn Host, question: &QueryContext) -> Known<Vec<PathBuf>> {
    Inputs { resolver: Resolver { host, settings }, question, libraries: Vec::new(), sources: Vec::new() }.collect().map_or(Known::Unknown, Known::Yes)
}

/// What [`program_inputs`] depends on besides `settings`: the program's
/// configuration file, else (none found) the program directory. Files of
/// one project share it, so their input list is collected once.
pub fn program_anchor(settings: &Settings, host: &dyn Host, question: &QueryContext) -> Known<PathBuf> {
    let resolver = Resolver { host, settings };
    let (Ok(directory), Ok(config)) = (resolver.program_directory(), resolver.configuration(question)) else { return Known::Unknown };
    Known::Yes(config.path.clone().unwrap_or(directory))
}

struct Inputs<'a> {
    resolver: Resolver<'a, dyn Host + 'a>,
    question: &'a QueryContext,
    libraries: Vec<PathBuf>,
    sources: Vec<PathBuf>,
}

enum Visit {
    Enter(Configuration, PathBuf),
    Exit(PathBuf),
}

impl Inputs<'_> {
    fn collect(mut self) -> Result<Vec<PathBuf>, ()> {
        let directory = self.resolver.program_directory()?;
        let config = Configuration::load(&self.resolver, &directory, self.question)?;
        let mut active = BTreeSet::new();
        let mut complete = BTreeSet::new();
        let mut visits = vec![Visit::Enter(config, directory)];
        while let Some(visit) = visits.pop() {
            spend(self.question)?;
            let (config, directory) = match visit {
                Visit::Enter(config, directory) => (config, directory),
                Visit::Exit(path) => { active.remove(&path); complete.insert(path); continue; }
            };
            if let Some(path) = &config.path {
                if active.contains(path) { return Err(()) }
                if complete.contains(path) { continue; }
                active.insert(path.clone());
                visits.push(Visit::Exit(path.clone()));
            }
            let directory = config.path.as_deref().and_then(Path::parent).unwrap_or(&directory);
            self.libraries(&config, directory)?;
            self.type_roots(&config, directory)?;
            self.project(&config, directory)?;
            for reference in config.references.iter().rev() {
                let path = self.reference(reference)?;
                let child = Configuration::at(&self.resolver, path, self.question)?;
                visits.push(Visit::Enter(child, reference.clone()));
            }
        }
        let mut seen = BTreeSet::new();
        Ok(self.libraries.into_iter().chain(self.sources).filter(|path| seen.insert(path.clone())).collect())
    }

    fn reference(&self, reference: &Path) -> Result<PathBuf, ()> {
        if self.resolver.host.is_file(reference, self.question).known().ok_or(())? { return Ok(reference.to_path_buf()) }
        for name in &self.resolver.settings.config.files {
            spend(self.question)?;
            let path = reference.join(&**name);
            if self.resolver.host.is_file(&path, self.question).known().ok_or(())? { return Ok(path) }
        }
        Err(())
    }

    fn libraries(&mut self, config: &Configuration, directory: &Path) -> Result<(), ()> {
        let settings = self.resolver.settings;
        let names = config.options.get(&*settings.globals.library_option).map_or_else(
            || Ok(settings.globals.default_libraries.clone()), |option| texts(&option.value)
        )?;
        for name in names {
            let mut found = None;
            'ancestors: for ancestor in directory.ancestors() {
                for package in &settings.packages.directories {
                    spend(self.question)?;
                    let package_dir = ancestor.join(&**package);
                    let path = clean(Path::new(&expand(&settings.globals.libraries, &[("package_dir", &package_dir.to_string_lossy()), ("name", &name)])?));
                    let request = DiscoveryRequest {
                        root: path.parent().ok_or(())?.to_path_buf(),
                        include: vec![path.file_name().and_then(|name| name.to_str()).ok_or(())?.into()],
                        exclude: Vec::new(), extensions: Vec::new(),
                    };
                    if self.resolver.host.discover(&request, self.question).known().ok_or(())?.contains(&path) {
                        found = Some(path);
                        break 'ancestors;
                    }
                }
            }
            self.libraries.push(found.ok_or(())?);
        }
        Ok(())
    }

    fn type_roots(&mut self, config: &Configuration, directory: &Path) -> Result<(), ()> {
        let settings = self.resolver.settings;
        let names = config.options.get(&*settings.globals.types_option).map(|option| texts(&option.value)).transpose()?;
        if names.as_ref().is_some_and(Vec::is_empty) { return Ok(()) }
        let roots = match config.options.get(&*settings.globals.type_roots_option) {
            Some(option) => texts(&option.value)?.iter().map(|root| clean(&option.directory.join(&**root))).collect(),
            None => directory.ancestors().flat_map(|directory| settings.globals.type_directories.iter().map(move |root| clean(&directory.join(&**root)))).collect::<Vec<_>>(),
        };
        if let Some(names) = names {
            for name in names {
                let name = name.strip_prefix(&*settings.packages.scoped_prefix).filter(|_| !settings.packages.scoped_prefix.is_empty()).map_or_else(
                    || name.to_string(), |name| name.replace('/', &settings.packages.type_join)
                );
                let mut found = None;
                for root in &roots {
                    spend(self.question)?;
                    match self.resolver.package_path(&root.join(&name), ".", &[], self.question)? {
                        Probe::Found(path) => { found = Some(path); break; }
                        Probe::Blocked => return Err(()),
                        Probe::Missing => {}
                    }
                }
                if let Some(path) = found {
                    self.libraries.push(path);
                }
            }
        } else {
            let mut seen = BTreeSet::new();
            for root in roots {
                spend(self.question)?;
                let mut extensions = settings.extensions.clone();
                if !settings.packages.metadata.is_empty() { extensions.push(settings.packages.metadata.clone()); }
                let request = DiscoveryRequest { root: root.clone(), include: vec!["**/*".into()], exclude: Vec::new(), extensions };
                let paths = self.resolver.host.discover(&request, self.question).known().ok_or(())?;
                let mut packages = BTreeSet::new();
                for path in paths {
                    spend(self.question)?;
                    let relative = path.strip_prefix(&root).map_err(|_| ())?;
                    if relative.components().count() > 1 {
                        let name = relative.components().next().ok_or(())?.as_os_str().to_owned();
                        packages.insert(name);
                    }
                }
                for name in packages {
                    if !seen.insert(name.clone()) { continue; }
                    match self.resolver.package_path(&root.join(name), ".", &[], self.question)? {
                        Probe::Found(path) => self.libraries.push(path),
                        Probe::Blocked | Probe::Missing => return Err(()),
                    }
                }
            }
        }
        Ok(())
    }

    fn project(&mut self, config: &Configuration, directory: &Path) -> Result<(), ()> {
        let settings = self.resolver.settings;
        let files = config.fields.get(&*settings.config.source_files);
        if let Some(files) = files {
            for file in texts(&files.value)? {
                spend(self.question)?;
                let path = clean(&files.directory.join(&*file));
                if !self.resolver.host.is_file(&path, self.question).known().ok_or(())? { return Err(()) }
                self.sources.push(path);
            }
        }
        let include = config.fields.get(&*settings.config.include);
        let (root, include) = match include {
            Some(option) => (option.directory.clone(), texts(&option.value)?),
            None if files.is_some() => return Ok(()),
            None => (directory.to_path_buf(), settings.globals.project_files.clone()),
        };
        if include.is_empty() { return Ok(()) }
        let exclude = config.fields.get(&*settings.config.exclude).map(|option| globs_relative(option, &root)).transpose()?.unwrap_or_default();
        let request = DiscoveryRequest { root, include, exclude, extensions: settings.extensions.clone() };
        self.sources.extend(self.resolver.host.discover(&request, self.question).known().ok_or(())?);
        Ok(())
    }
}

fn texts(value: &Json) -> Result<Vec<Box<str>>, ()> {
    value.array().ok_or(())?.iter().map(|value| value.text().map(Into::into).ok_or(())).collect()
}

fn globs_relative(option: &OptionValue, root: &Path) -> Result<Vec<Box<str>>, ()> {
    Ok(texts(&option.value)?.into_iter().filter_map(|glob| {
        let path = clean(&option.directory.join(&*glob));
        path.strip_prefix(root).ok().map(|path| path.to_string_lossy().replace('\\', "/").into())
    }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::resolution::{Config, DiscoveryRequest, Globals, Packages};
    use super::super::super::question::ScopeRef;
    use std::collections::BTreeMap;
    use std::path::Path;

    struct Files {
        text: BTreeMap<PathBuf, String>,
        discovered: BTreeMap<DiscoveryRequest, Vec<PathBuf>>,
    }

    const PROGRAM_FILE: u32 = 7;

    impl Host for Files {
        fn program(&self) -> Known<u32> { Known::Yes(PROGRAM_FILE) }
        fn path(&self, file: u32) -> Known<PathBuf> {
            Known::Yes(if file == PROGRAM_FILE { "/project/src/main.ts".into() } else { "/foreign/library.d.ts".into() })
        }
        fn text(&self, path: &Path, _: &QueryContext) -> Known<Option<String>> { Known::Yes(self.text.get(path).cloned()) }
        fn is_file(&self, path: &Path, _: &QueryContext) -> Known<bool> { Known::Yes(self.text.contains_key(path)) }
        fn scope(&self, _: &Path, _: &QueryContext) -> Known<Option<ScopeRef>> { Known::Yes(None) }
        fn ambient(&self, _: u32, _: &str, _: u64, _: &QueryContext) -> Known<Option<ScopeRef>> { Known::Yes(None) }
        fn discover(&self, request: &DiscoveryRequest, _: &QueryContext) -> Known<Vec<PathBuf>> {
            if let Some(paths) = self.discovered.get(request) { return Known::Yes(paths.clone()) }
            if let [name] = &*request.include {
                if request.extensions.is_empty() && !name.contains(['*', '?', '[', '{']) {
                    let path = request.root.join(&**name);
                    return Known::Yes(self.text.contains_key(&path).then_some(path).into_iter().collect());
                }
            }
            Known::Unknown
        }
    }

    fn settings() -> Settings {
        Settings {
            config: Config {
                files: vec!["tsconfig.json".into()], source_files: "files".into(), include: "include".into(), exclude: "exclude".into(),
                extends: "extends".into(), references: "references[].path".into(), options: "compilerOptions".into(), ..Config::default()
            },
            packages: Packages { directories: vec!["node_modules".into()], ..Packages::default() },
            globals: Globals {
                libraries: "{package_dir}/typescript/lib/lib.{name}.d.ts".into(), library_option: "lib".into(),
                default_libraries: vec!["base".into()], types_option: "types".into(), type_roots_option: "typeRoots".into(),
                references: true, ..Globals::default()
            },
            extensions: vec![".ts".into()],
            candidates: vec!["{dir}/{name}{ext}".into(), "{dir}/{name}/index{ext}".into()],
            ..Settings::default()
        }
    }

    #[test]
    fn inherited_file_origins_and_referenced_projects_survive_child_overrides() {
        let settings = settings();
        let request = DiscoveryRequest { root: "/base".into(), include: vec!["globals/**/*.ts".into()], exclude: vec!["skip/**".into()], extensions: settings.extensions.clone() };
        let files = Files {
            text: BTreeMap::from([
                ("/base/tsconfig.json".into(), r#"{"files":["explicit.ts"],"include":["globals/**/*.ts"],"exclude":["skip/**"],"references":[{"path":"not-inherited"}]}"#.into()),
                ("/project/tsconfig.json".into(), r#"{"extends":"../base/tsconfig.json","compilerOptions":{"lib":[],"types":[]},"references":[{"path":"../referenced"}]}"#.into()),
                ("/base/explicit.ts".into(), "".into()),
                ("/referenced/tsconfig.json".into(), r#"{"files":["ambient.ts"],"compilerOptions":{"lib":[],"types":[]}}"#.into()),
                ("/referenced/ambient.ts".into(), "".into()),
            ]),
            discovered: BTreeMap::from([(request, vec!["/base/globals/a.ts".into()])]),
        };
        assert_eq!(program_inputs(&settings, &files, &QueryContext::default()), Known::Yes(vec![
            "/base/explicit.ts".into(), "/base/globals/a.ts".into(), "/referenced/ambient.ts".into()
        ]));
    }

    #[test]
    fn explicit_empty_library_and_type_lists_suppress_default_roots() {
        let settings = settings();
        let files = Files {
            text: BTreeMap::from([
                ("/project/tsconfig.json".into(), r#"{"files":[],"compilerOptions":{"lib":[],"types":[]}}"#.into()),
                ("/project/node_modules/typescript/lib/lib.base.d.ts".into(), "".into()),
            ]),
            discovered: BTreeMap::new(),
        };
        assert_eq!(program_inputs(&settings, &files, &QueryContext::default()), Known::Yes(Vec::new()));
    }

    #[test]
    fn library_membership_comes_from_discovery_and_missing_libraries_remain_unknown() {
        let mut settings = settings();
        settings.globals.default_libraries = vec!["BASE".into()];
        settings.globals.libraries = "{package_dir}/typescript/lib/lib.{name:lower}.d.ts".into();
        let path: PathBuf = "/project/node_modules/typescript/lib/lib.base.d.ts".into();
        let request = DiscoveryRequest {
            root: path.parent().unwrap().into(), include: vec!["lib.base.d.ts".into()],
            exclude: Vec::new(), extensions: Vec::new(),
        };
        let mut files = Files {
            text: BTreeMap::from([("/project/tsconfig.json".into(), r#"{"files":[],"compilerOptions":{"types":[]}}"#.into())]),
            discovered: BTreeMap::from([(request.clone(), vec![path.clone()])]),
        };
        assert_eq!(program_inputs(&settings, &files, &QueryContext::default()), Known::Yes(vec![path]));
        files.discovered.insert(request, Vec::new());
        assert_eq!(program_inputs(&settings, &files, &QueryContext::default()), Known::Unknown);
    }

    #[test]
    fn program_inputs_use_the_host_program_not_file_zero() {
        let settings = settings();
        let files = Files {
            text: BTreeMap::from([
                ("/project/tsconfig.json".into(), r#"{"files":["own.ts"],"compilerOptions":{"lib":[],"types":[]}}"#.into()),
                ("/project/own.ts".into(), "".into()),
                ("/foreign/tsconfig.json".into(), r#"{"files":["unrelated.ts"],"compilerOptions":{"lib":[],"types":[]}}"#.into()),
                ("/foreign/unrelated.ts".into(), "".into()),
            ]),
            discovered: BTreeMap::new(),
        };
        assert_eq!(program_inputs(&settings, &files, &QueryContext::default()), Known::Yes(vec!["/project/own.ts".into()]));
    }

    #[test]
    fn libraries_precede_config_relative_type_roots_and_project_files() {
        let settings = settings();
        let request = DiscoveryRequest { root: "/base/types".into(), include: vec!["**/*".into()], exclude: Vec::new(), extensions: settings.extensions.clone() };
        let files = Files {
            text: BTreeMap::from([
                ("/base/tsconfig.json".into(), r#"{"compilerOptions":{"typeRoots":["types"]}}"#.into()),
                ("/project/tsconfig.json".into(), r#"{"extends":"../base/tsconfig.json","files":["own.ts"]}"#.into()),
                ("/project/node_modules/typescript/lib/lib.base.d.ts".into(), "".into()),
                ("/project/own.ts".into(), "".into()),
                ("/base/types/ambient/index.ts".into(), "".into()),
            ]),
            discovered: BTreeMap::from([(request, vec!["/base/types/ambient/index.ts".into()])]),
        };
        assert_eq!(program_inputs(&settings, &files, &QueryContext::default()), Known::Yes(vec![
            "/project/node_modules/typescript/lib/lib.base.d.ts".into(), "/base/types/ambient/index.ts".into(), "/project/own.ts".into()
        ]));
    }

    #[test]
    fn missing_discovery_and_reference_cycles_are_unknown_not_partial_success() {
        let settings = settings();
        for config in [
            r#"{"include":["**/*.ts"],"compilerOptions":{"lib":[],"types":[]}}"#,
            r#"{"files":[],"references":[{"path":"."}],"compilerOptions":{"lib":[],"types":[]}}"#,
            r#"{"files":[],"references":[true],"compilerOptions":{"lib":[],"types":[]}}"#,
        ] {
            let files = Files { text: BTreeMap::from([("/project/tsconfig.json".into(), config.into())]), discovered: BTreeMap::new() };
            assert_eq!(program_inputs(&settings, &files, &QueryContext::default()), Known::Unknown);
        }
    }

    #[test]
    fn selected_type_packages_load_only_their_declared_entry_and_record_missing_entries() {
        let mut settings = settings();
        settings.packages.metadata = "package.json".into();
        settings.packages.entries = vec!["types".into()];
        settings.candidates = vec!["{dir}/{name}{ext}".into(), "{dir}/{name}/index{ext}".into()];
        settings.extensions = vec![".d.ts".into()];
        let request = DiscoveryRequest { root: "/project/types".into(), include: vec!["**/*".into()], exclude: Vec::new(), extensions: vec![".d.ts".into(), "package.json".into()] };
        let mut files = Files {
            text: BTreeMap::from([
                ("/project/tsconfig.json".into(), r#"{"files":[],"compilerOptions":{"lib":[],"types":["missing","one","also-missing"],"typeRoots":["types"]}}"#.into()),
                ("/project/types/one/package.json".into(), r#"{"types":"entry.d.ts"}"#.into()),
                ("/project/types/one/entry.d.ts".into(), "".into()),
                ("/project/types/one/unrelated-global.d.ts".into(), "".into()),
            ]),
            discovered: BTreeMap::from([(request, vec!["/project/types/one/entry.d.ts".into(), "/project/types/one/unrelated-global.d.ts".into()])]),
        };
        assert_eq!(program_inputs(&settings, &files, &QueryContext::default()), Known::Yes(vec!["/project/types/one/entry.d.ts".into()]));
        files.text.insert("/project/tsconfig.json".into(), r#"{"files":[],"compilerOptions":{"lib":[],"typeRoots":["types"]}}"#.into());
        assert_eq!(program_inputs(&settings, &files, &QueryContext::default()), Known::Yes(vec!["/project/types/one/entry.d.ts".into()]));
        files.text.remove(&PathBuf::from("/project/types/one/entry.d.ts"));
        for paths in files.discovered.values_mut() { paths.retain(|path| path != Path::new("/project/types/one/entry.d.ts")); }
        assert_eq!(program_inputs(&settings, &files, &QueryContext::default()), Known::Unknown);
    }
}
