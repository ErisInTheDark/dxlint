//! Top/bottom and literal constituents that override other type information.

use super::*;
use super::constituents::bare;
use std::collections::BTreeMap;

struct Part {
    primitive: Option<&'static str>,
    literal: bool,
    name: String,
    special: Option<&'static str>,
}

impl Part {
    fn other() -> Part {
        Part { primitive: None, literal: false, name: "literal type".into(), special: None }
    }
}

fn parts(file: &NativeFile<'_>, node: u32) -> Vec<Part> {
    let node = bare(file, node);
    let s = &file.syntax;
    if s.is(node, Kind::Union) {
        return s.all(node, Field::Type).flat_map(|part| parts(file, part)).collect();
    }
    if super::unsafe_types::missing_annotation(file, node) {
        return vec![Part { primitive: None, literal: false, name: s.text(node).to_string(), special: Some("error") }];
    }
    if s.is(node, Kind::TemplateType) {
        return vec![Part { primitive: Some("string"), literal: true, name: "template literal type".into(), special: None }];
    }
    fn typed(ty: Ty) -> Vec<Part> {
        let special = match ty { Ty::Any => Some("any"), Ty::Never => Some("never"), _ => None };
        if let Some(special) = special {
            return vec![Part { primitive: None, literal: false, name: special.into(), special: Some(special) }];
        }
        match ty {
            Ty::Prim(p @ ("string" | "number" | "boolean" | "bigint")) =>
                vec![Part { primitive: Some(p), literal: false, name: p.into(), special: None }],
            Ty::Lit(text, p @ ("string" | "number" | "boolean" | "bigint")) => {
                let name = match p {
                    "string" => crate::expr::js::strings::decode_literal(&text).and_then(|text| serde_json::to_string(&text).ok()),
                    "number" => crate::native::ts::statics::number(&text).map(crate::expr::value::js_number_text),
                    _ => Some(text.to_string()),
                };
                name.map_or_else(|| vec![Part::other()], |name| vec![Part { primitive: Some(p), literal: true, name, special: None }])
            }
            Ty::Union(types) => {
                if types.len() == 2 && types.iter().all(|ty| matches!(ty, Ty::Lit(_, "boolean"))) && !types[0].same_identity(&types[1]) {
                    typed(Ty::Prim("boolean"))
                } else {
                    types.into_iter().flat_map(typed).collect()
                }
            }
            _ => vec![Part::other()],
        }
    }
    if s.is(node, Kind::TypeKeyword) && s.text(node) == "unknown" || matches!(file.type_of(node), Ty::Unknown) && s.is(node, Kind::TypeRef) && s.text(node) == "unknown" {
        return vec![Part { primitive: None, literal: false, name: "unknown".into(), special: Some("unknown") }];
    }
    typed(file.type_of(node))
}

fn finding(file: &NativeFile<'_>, node: u32, mode: &str, name: &str, primitive: &str, container: &str) -> Value<'static> {
    let node = bare(file, node);
    Value::record(vec![
        ("offset".into(), Value::from(file.syntax.tree.node(node).start as usize)),
        ("mode".into(), Value::from(mode.to_string())),
        ("name".into(), Value::from(name.to_string())),
        ("primitive".into(), Value::from(primitive.to_string())),
        ("container".into(), Value::from(container.to_string())),
    ])
}

pub(super) fn findings(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let union = s.is(node, Kind::Union);
    if !union && !s.is(node, Kind::Intersection) { return Value::Null }
    let container = if union { "union" } else { "intersection" };
    let return_type = s.field(node) == Some(Field::ReturnType);
    let entries: Vec<_> = s.all(node, Field::Type).map(|node| (node, parts(file, node))).collect();
    let mut out = Vec::new();
    let mut primitives = BTreeMap::<&str, Vec<u32>>::new();
    let mut literals = BTreeMap::<&str, Vec<(u32, &str)>>::new();
    let mut unions = Vec::new();
    for (node, parts) in &entries {
        if parts.len() >= 2 { unions.push((*node, parts)) }
        for part in parts {
            let mode = match part.special {
                Some("error") => Some("error"),
                Some("any") => Some("overrides"),
                Some("unknown") => Some(if union { "overrides" } else { "overridden" }),
                Some("never") if !union => Some("overrides"),
                Some("never") if !return_type => Some("overridden"),
                _ => None,
            };
            if let Some(mode) = mode {
                out.push(finding(file, *node, mode, &part.name, "", container));
            } else if let Some(primitive) = part.primitive {
                if part.literal {
                    literals.entry(primitive).or_default().push((*node, &part.name));
                } else {
                    primitives.entry(primitive).or_default().push(*node);
                }
            }
        }
    }
    if union {
        let mut groups = BTreeMap::<(u32, &str), Vec<&str>>::new();
        for (primitive, entries) in literals {
            if primitives.contains_key(primitive) {
                for (node, name) in entries { groups.entry((node, primitive)).or_default().push(name) }
            }
        }
        for ((node, primitive), names) in groups {
            out.push(finding(file, node, "literal", &names.join(" | "), primitive, container));
        }
    } else if !unions.is_empty() {
        for (node, parts) in unions {
            if parts.iter().all(|part| part.literal && part.primitive.is_some_and(|p| primitives.contains_key(p))) {
                let names: Vec<_> = parts.iter().map(|part| part.name.as_str()).collect();
                out.push(finding(file, node, "primitive", &names.join(" | "), parts.last().and_then(|part| part.primitive).unwrap_or(""), container));
            }
        }
    } else {
        for (primitive, nodes) in primitives {
            if let Some(literals) = literals.get(primitive) {
                let names: Vec<_> = literals.iter().map(|(_, name)| *name).collect();
                for node in nodes {
                    out.push(finding(file, node, "primitive", &names.join(" | "), primitive, container));
                }
            }
        }
    }
    Value::list(out)
}
