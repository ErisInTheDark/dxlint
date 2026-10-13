use super::configuration::Configuration;
use super::metadata::Json;
use super::resolution::{spend, Host, Resolver};
use super::super::question::{Known, QueryContext};
use crate::expr::Value;

pub fn context<H: Host + ?Sized>(resolver: &Resolver<'_, H>, importer: u32, question: &QueryContext) -> Known<Value<'static>> {
    context_inner(resolver, importer, question).map_or(Known::Unknown, Known::Yes)
}

fn context_inner<H: Host + ?Sized>(resolver: &Resolver<'_, H>, importer: u32, question: &QueryContext) -> Result<Value<'static>, ()> {
    let path = resolver.host.path(importer).known().ok_or(())?;
    let directory = path.parent().ok_or(())?;
    let config = Configuration::load(resolver, &resolver.program_directory()?, question)?;
    let config = Json::Object(config.options.into_iter().map(|(key, value)| (key, value.value)).collect()).into_value(question)?;
    let mut package = Value::Null;
    if !resolver.settings.packages.metadata.is_empty() {
        for ancestor in directory.ancestors() {
            spend(question)?;
            if let Some(metadata) = resolver.metadata(&ancestor.join(&*resolver.settings.packages.metadata), question)? {
                package = metadata.into_value(question)?;
                break;
            }
        }
    }
    let conditions = Value::list(resolver.settings.conditions.iter().map(|condition| Value::Str(condition.to_string().into())).collect());
    Ok(Value::record(vec![("config".into(), config), ("package".into(), package), ("conditions".into(), conditions)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::resolution::{Config, Packages, Settings};
    use super::super::super::question::ScopeRef;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    struct Files(BTreeMap<PathBuf, String>);

    impl Host for Files {
        fn program(&self) -> Known<u32> { Known::Yes(0) }
        fn path(&self, file: u32) -> Known<PathBuf> {
            Known::Yes(if file == 0 { "/project/src/main.ts".into() } else { "/foreign/module.ts".into() })
        }
        fn text(&self, path: &Path, _: &QueryContext) -> Known<Option<String>> { Known::Yes(self.0.get(path).cloned()) }
        fn is_file(&self, path: &Path, _: &QueryContext) -> Known<bool> { Known::Yes(self.0.contains_key(path)) }
        fn scope(&self, _: &Path, _: &QueryContext) -> Known<Option<ScopeRef>> { Known::Yes(None) }
        fn ambient(&self, _: u32, _: &str, _: u64, _: &QueryContext) -> Known<Option<ScopeRef>> { Known::Yes(None) }
    }

    fn settings() -> Settings {
        Settings {
            config: Config { files: vec!["tsconfig.json".into()], extends: "extends".into(), options: "compilerOptions".into(), ..Config::default() },
            packages: Packages { metadata: "package.json".into(), ..Packages::default() },
            conditions: vec!["custom".into()],
            ..Settings::default()
        }
    }

    #[test]
    fn mode_context_uses_effective_options_and_the_nearest_package_without_losing_scalars() {
        let files = Files(BTreeMap::from([
            (PathBuf::from("/base/tsconfig.json"), r#"{"compilerOptions":{"enabled":true,"level":2024,"untouched":"yes"}}"#.into()),
            (PathBuf::from("/project/tsconfig.json"), r#"{"extends":"../base/tsconfig.json","compilerOptions":{"level":2025}}"#.into()),
            (PathBuf::from("/project/package.json"), r#"{"type":"near"}"#.into()),
            (PathBuf::from("/package.json"), r#"{"type":"far"}"#.into()),
        ]));
        let settings = settings();
        let Known::Yes(value) = Resolver { host: &files, settings: &settings }.context(0, &QueryContext::default()) else {
            panic!("mode context must be available");
        };
        let Value::Record(root) = value else { panic!("mode context must be a record") };
        let field = |name: &str| root.iter().find(|(key, _)| &**key == name).map(|(_, value)| value).unwrap();
        let Value::Record(config) = field("config") else { panic!("options must be available") };
        let option = |name: &str| config.iter().find(|(key, _)| &**key == name).map(|(_, value)| value).unwrap();
        assert_eq!(option("enabled"), &Value::Bool(true));
        assert_eq!(option("level"), &Value::Num(2025.0));
        assert_eq!(option("untouched"), &Value::from("yes"));
        let Value::Record(package) = field("package") else { panic!("nearest package must be available") };
        assert!(package.iter().any(|(key, value)| &**key == "type" && value == &Value::from("near")));
        assert_eq!(field("conditions"), &Value::list(vec![Value::from("custom")]));
    }

    #[test]
    fn foreign_context_keeps_program_options_and_its_own_package() {
        let files = Files(BTreeMap::from([
            (PathBuf::from("/project/tsconfig.json"), r#"{"compilerOptions":{"level":2025}}"#.into()),
            (PathBuf::from("/foreign/tsconfig.json"), r#"{"compilerOptions":{"level":1999}}"#.into()),
            (PathBuf::from("/foreign/package.json"), r#"{"type":"module"}"#.into()),
        ]));
        let settings = settings();
        let Known::Yes(Value::Record(context)) = Resolver { host: &files, settings: &settings }.context(1, &QueryContext::default()) else {
            panic!("foreign context must be known");
        };
        let field = |name: &str| context.iter().find(|(key, _)| &**key == name).map(|(_, value)| value).unwrap();
        let Value::Record(config) = field("config") else { panic!("program options must be available") };
        assert!(config.iter().any(|(key, value)| &**key == "level" && value == &Value::Num(2025.0)));
        let Value::Record(package) = field("package") else { panic!("source package must be available") };
        assert!(package.iter().any(|(key, value)| &**key == "type" && value == &Value::from("module")));
    }

    #[test]
    fn malformed_or_cyclic_configurations_do_not_become_empty_mode_contexts() {
        for source in ["{broken", "[1]", r#"{"extends":"./tsconfig.json"}"#] {
            let files = Files(BTreeMap::from([(PathBuf::from("/project/tsconfig.json"), source.into())]));
            let settings = settings();
            assert_eq!(Resolver { host: &files, settings: &settings }.context(0, &QueryContext::default()), Known::Unknown);
        }
    }
}
