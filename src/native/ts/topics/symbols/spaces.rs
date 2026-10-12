use crate::expr::Value;
use crate::native::ts::facts::{Facts, NativeFile};
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::modules::SymbolSpaces;

fn name(file: &NativeFile<'_>, node: u32) -> String {
    file.static_name(node).unwrap_or_else(|| crate::native::ts::statics::identifier_name(file.syntax.text(node)))
}

fn specifier(file: &NativeFile<'_>, node: u32) -> Option<u32> {
    let s = &file.syntax;
    if s.is(node, K::ExportSpec) { Some(node) } else { s.parent(node).filter(|&parent| s.is(parent, K::ExportSpec)) }
}

pub(super) fn remote_export(file: &NativeFile<'_>, node: u32) -> Option<(String, String)> {
    let s = &file.syntax;
    let specifier = specifier(file, node)?;
    let export = s.parent(specifier)?;
    let source = s.get(export, F::From)?;
    Some((name(file, source), name(file, s.get(specifier, F::Local)?)))
}

fn record(spaces: SymbolSpaces) -> Value<'static> {
    Value::record(vec![
        ("type".into(), Value::Bool(spaces.type_space)),
        ("value".into(), Value::Bool(spaces.value)),
        ("namespace".into(), Value::Bool(spaces.namespace)),
    ])
}

pub(super) fn symbol(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let ck = &file.typing().checker;
    let specifier = specifier(file, node);
    let local = specifier.and_then(|specifier| s.get(specifier, F::Local)).unwrap_or(node);
    let mut spaces = if let Some((source, name)) = remote_export(file, node) {
        file.project.resolve(ck.file, &source).and_then(|module| file.project.export_spaces(module, &name))
    } else if s.is(local, K::Ident) {
        file.project.declaration_spaces(&ck.lookup(&file.env_at(local), &name(file, local)))
    } else { None };
    let type_only = specifier.is_some_and(|specifier| {
        s.word(specifier, F::Kind) == Some("type") || s.parent(specifier).is_some_and(|export| s.word(export, F::Kind) == Some("type"))
    });
    if type_only { if let Some(spaces) = spaces.as_mut() { spaces.value = false; } }
    spaces.map_or(Value::Null, record)
}

pub(super) fn module(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let source = s.get(node, F::From).or_else(|| {
        s.parent(node).filter(|&parent| matches!(s.kind(parent), Some(K::Import | K::Export)) && s.get(parent, F::From) == Some(node)).map(|_| node)
    });
    let Some(source) = source else { return Value::Null };
    let ck = &file.typing().checker;
    let Some(symbols) = file.project.resolve(ck.file, &name(file, source)).and_then(|module| file.project.module_symbols(module)) else { return Value::Null };
    Value::record(vec![
        ("complete".into(), Value::Bool(symbols.complete)),
        ("exports".into(), Value::list(symbols.exports.into_iter().map(|(name, spaces)| Value::record(vec![
            ("name".into(), Value::text_copy(&name)),
            ("spaces".into(), spaces.map_or(Value::Null, record)),
        ])).collect())),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::ts::reads;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn field<'a>(value: &'a Value<'_>, name: &str) -> &'a Value<'a> {
        let Value::Record(fields) = value else { panic!("record expected: {value:?}") };
        fields.iter().find(|(key, _)| &**key == name).map(|(_, value)| value).expect("field")
    }

    struct Buffer(PathBuf);

    impl Drop for Buffer {
        fn drop(&mut self) { reads::drop_buffer(&self.0); }
    }

    #[test]
    fn reexports_use_the_remote_symbol_even_when_a_local_name_matches() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!("symbol-spaces-{}", NEXT.fetch_add(1, Ordering::Relaxed));
        let path = crate::config::source::normalize_path(&PathBuf::from(format!("/root/{name}.ts")));
        reads::set_buffer(&path, "export const T = 1;");
        let _buffer = Buffer(path);
        let source = format!("type T = string; export {{ /*@*/T }} from './{name}';");
        crate::engine::tests::with_native_file(&source, |file, nodes| {
            let spaces = symbol(file, nodes[0]);
            assert_eq!(field(&spaces, "value"), &Value::Bool(true));
            let metadata = super::super::metadata(file, nodes[0]);
            let Value::List(declarations) = field(&metadata, "declarations") else { panic!("declarations") };
            assert_eq!(field(declarations.first().expect("remote runtime declaration"), "kind"), &Value::from("variable"));
        });
    }
}
