//! Naming selectors and their semantic modifiers; policy stays in scoping.zlint.

use std::collections::HashSet;
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::types::Ty;

const SELECTORS: &[&str] = &[
    "variable", "function", "parameter", "parameterProperty", "classicAccessor", "enumMember",
    "classMethod", "objectLiteralMethod", "typeMethod", "classProperty", "objectLiteralProperty",
    "typeProperty", "autoAccessor", "class", "interface", "typeAlias", "enum", "typeParameter", "import",
];
const WEIGHTS: &[&str] = &[
    "const", "readonly", "static", "public", "protected", "private", "#private", "abstract",
    "destructured", "global", "exported", "unused", "requiresQuotes", "override", "async",
    "default", "namespace", "boolean", "string", "number", "function", "array",
];

struct Exported(HashSet<u32>);

fn exported(file: &NativeFile) -> Exported {
    let s = &file.syntax;
    Exported(file.bindings().refs.iter().filter_map(|(&reference, &declaration)| {
        s.parent(reference).is_some_and(|p| s.is(p, K::ExportSpec)).then_some(declaration)
    }).collect())
}

pub(super) fn candidate(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let b = file.bindings();
    let Some(parent) = s.parent(node) else { return Value::Null };
    let binding = b.decl_scope.contains_key(&node);
    let mut owner = parent;
    let mut modifiers = Vec::new();
    let selector;
    let name;
    if !binding && s.field(node) == Some(F::Key) || s.is(parent, K::EnumMember) && s.field(node) == Some(F::Name) {
        if s.is(node, K::Computed) { return Value::Null; }
        name = if s.is(node, K::Ident) { s.text(node).to_string() }
            else if s.is(node, K::PrivateName) { s.text(node).trim_start_matches('#').to_string() }
            else { crate::native::ts::statics::literal_text(s, node).unwrap_or_else(|| s.text(node).to_string()) };
        let class = s.parent(parent).is_some_and(|p| s.is(p, K::Class));
        let value = s.get(parent, F::Value).or_else(|| s.get(parent, F::Init));
        let function = value.is_some_and(|n| matches!(s.kind(n), Some(K::Function | K::Arrow)));
        let accessor = s.has_word(parent, F::Modifier, "accessor");
        let getter = matches!(s.word(parent, F::Kind), Some("get" | "set"));
        selector = match s.kind(parent) {
            Some(K::EnumMember) => "enumMember",
            Some(K::Method) if getter => "classicAccessor",
            Some(K::Method) if class => {
                if name == "constructor" { return Value::Null; }
                "classMethod"
            }
            Some(K::Method) => "objectLiteralMethod",
            Some(K::ClassProp) if accessor => "autoAccessor",
            Some(K::ClassProp) if function => "classMethod",
            Some(K::ClassProp) => "classProperty",
            Some(K::Prop) if function => "objectLiteralMethod",
            Some(K::Prop) => "objectLiteralProperty",
            Some(K::MethodSig) => "typeMethod",
            Some(K::PropSig) => {
                if s.get(parent, F::Type).is_some_and(|n| s.is(n, K::FnType)) { "typeMethod" } else { "typeProperty" }
            }
            _ => return Value::Null,
        };
        if selector != "enumMember" {
            let visibility = if s.is(node, K::PrivateName) { "#private" }
                else if s.has_word(parent, F::Modifier, "private") { "private" }
                else if s.has_word(parent, F::Modifier, "protected") { "protected" } else { "public" };
            modifiers.push(visibility);
        }
        if !identifier_name(&name) { modifiers.push("requiresQuotes"); }
        if value.is_some_and(|n| s.has_word(n, F::Modifier, "async")) { modifiers.push("async"); }
    } else if binding {
        name = s.text(node).to_string();
        while !matches!(s.kind(owner), Some(K::Declarator | K::Param | K::Function | K::Class | K::Interface | K::TypeAlias | K::Enum | K::TypeParam | K::Import | K::ImportSpec | K::Program)) {
            let Some(p) = s.parent(owner) else { return Value::Null };
            owner = p;
        }
        selector = match s.kind(owner) {
            Some(K::Declarator) => "variable",
            Some(K::Param) if s.all(owner, F::Modifier).any(|n| matches!(s.text(n), "public" | "protected" | "private" | "readonly")) => "parameterProperty",
            Some(K::Param) => "parameter",
            Some(K::Function) => "function",
            Some(K::Class) => "class",
            Some(K::Interface) => "interface",
            Some(K::TypeAlias) => "typeAlias",
            Some(K::Enum) => "enum",
            Some(K::TypeParam) => "typeParameter",
            Some(K::Import) => {
                modifiers.push(if s.field(node) == Some(F::Namespace) { "namespace" } else { "default" });
                "import"
            }
            Some(K::ImportSpec) if s.word(owner, F::Imported) == Some("default") => { modifiers.push("default"); "import" }
            _ => return Value::Null,
        };
        if selector == "variable" {
            if s.parent(owner).is_some_and(|p| s.word(p, F::Kind) == Some("const")) { modifiers.push("const"); }
            if s.get(owner, F::Init).is_some_and(|n| s.has_word(n, F::Modifier, "async")) { modifiers.push("async"); }
        }
        if matches!(selector, "variable" | "function") && b.decl_scope.get(&node) == Some(&0) { modifiers.push("global"); }
        let canonical = b.merged.get(&node).copied().unwrap_or(node);
        if let Some(binding) = b.decls.get(&canonical) {
            let exported = binding.exported || file.cached(exported).0.contains(&canonical);
            if exported { modifiers.push("exported"); }
            let unused = match crate::native::ts::unused::var_fact(file, canonical) {
                Value::Record(fields) => fields.iter().any(|(name, value)| name.as_ref() == "used" && matches!(value, Value::Bool(false))),
                _ => binding.uses == 0 && !exported,
            };
            if unused { modifiers.push("unused"); }
        }
        if selector == "parameterProperty" && !s.has_word(owner, F::Modifier, "private") && !s.has_word(owner, F::Modifier, "protected") { modifiers.push("public"); }
        let mut pattern = node;
        while pattern != owner {
            let Some(p) = s.parent(pattern) else { break };
            if s.is(p, K::PatternProp) {
                if s.get(p, F::Key).is_some_and(|key| s.text(key) == name) { modifiers.push("destructured"); }
                break;
            }
            pattern = p;
        }
    } else { return Value::Null; }
    for &modifier in WEIGHTS {
        if s.has_word(owner, F::Modifier, modifier) && !modifiers.contains(&modifier) { modifiers.push(modifier); }
    }
    let group = match selector {
        "variable" | "function" | "parameter" => "variableLike",
        "class" | "interface" | "typeAlias" | "enum" | "typeParameter" => "typeLike",
        "import" => "",
        _ => "memberLike",
    };
    let subgroup = match selector {
        "classMethod" | "objectLiteralMethod" | "typeMethod" => "method",
        "classProperty" | "objectLiteralProperty" | "typeProperty" => "property",
        "classicAccessor" | "autoAccessor" => "accessor",
        _ => "",
    };
    let label = selector.chars().enumerate().flat_map(|(i, c)| {
        if i == 0 { c.to_uppercase().collect::<Vec<_>>() }
        else if c.is_uppercase() { vec![' ', c] } else { vec![c] }
    }).collect::<String>();
    let selectors = vec![
        ("default", 0usize),
        (group, 1_000_000_000_000usize + match group { "variableLike" => 7, "typeLike" => 253952, "memberLike" => 8184, _ => 0 }),
        (subgroup, if subgroup == "accessor" { 1_000_000_000_000 + 4112 } else { 2_000_000_000_000 + if subgroup == "method" { 448 } else { 3584 } }),
        (selector, 3_000_000_000_000 + (1 << SELECTORS.iter().position(|&v| v == selector).unwrap_or(0))),
    ];
    Value::record(vec![
        ("name".into(), Value::from(name)),
        ("label".into(), Value::from(label)),
        ("typed".into(), Value::Bool(matches!(selector, "variable" | "parameter" | "classProperty" | "objectLiteralProperty" | "typeProperty" | "parameterProperty" | "classicAccessor"))),
        ("quotes".into(), Value::Bool(modifiers.contains(&"requiresQuotes"))),
        ("modifiers".into(), Value::list(modifiers.into_iter().map(Value::from).collect())),
        ("selectors".into(), Value::list(selectors.into_iter().filter(|(name, _)| !name.is_empty()).map(|(name, rank)| Value::record(vec![("name".into(), Value::from(name)), ("rank".into(), Value::from(rank))])).collect())),
        ("weights".into(), Value::list(WEIGHTS.iter().enumerate().map(|(i, &name)| Value::record(vec![("name".into(), Value::from(name)), ("rank".into(), Value::from(1usize << i))])).collect())),
    ])
}

pub(super) fn types(file: &NativeFile, node: u32) -> Value<'static> {
    let ty = file.type_of(node);
    Value::list(["boolean", "string", "number", "function", "array"].into_iter()
        .filter(|kind| matches_type(&ty, kind)).map(Value::from).collect())
}

fn identifier_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || matches!(c, '_' | '$'))
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '$'))
}

fn matches_type(ty: &Ty, kind: &str) -> bool {
    match ty {
        Ty::Prim(name) | Ty::Lit(_, name) => *name == kind,
        Ty::Array(_) | Ty::Tuple(_) => kind == "array",
        Ty::Fn(_) => kind == "function",
        Ty::Union(parts) => {
            let mut parts = parts.iter().filter(|t| !matches!(t, Ty::Null | Ty::Undefined));
            parts.clone().next().is_some() && parts.all(|t| matches_type(t, kind))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn typed_policy_precedes_general_policy_and_trims_before_formatting() {
        let l = native_linter(r#"rule naming-convention { set options [
            {selector: "variable", format: ["camelCase"]},
            {selector: "variable", types: ["boolean"], format: ["UPPER_CASE"], prefix: ["IS_"]}
        ] }"#);
        let path = Path::new("/root/a.ts");
        assert!(l.lint(path, "const IS_READY: boolean | null = true; const readyText: string = 'yes';").is_empty());
        assert_eq!(l.lint(path, "const ready: boolean = true;").len(), 1);
        assert_eq!(l.lint(path, "const IS_ready: boolean = true;").len(), 1);
    }
}
