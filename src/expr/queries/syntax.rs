use crate::expr::Value;
use crate::grammar::names::question::{Known, Leaf, Target};
use crate::grammar::names::semantic::Semantic;
use crate::grammar::syntax::ValueSource;

use super::QueryFunction;

pub(super) static FUNCTIONS: &[QueryFunction] = &[
    QueryFunction { name: "syntax_tree", arity: 3, call: parse },
];

fn parse(semantic: &Semantic<'_>, at: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let Target::Node(anchor) = at.target else { return Known::Unknown };
    let Some(source) = semantic.world.text(anchor.file) else { return Known::Unknown };
    let [Value::Str(name), input] = args else { return Known::Yes(error("input", "syntax_tree needs a parser name and input record")) };
    let Some(parser) = semantic.world.grammar().parsers.get(name) else {
        return Known::Yes(error("parser", &format!("unknown declared parser `{name}`")));
    };
    if let Some(Value::Str(text)) = field(input, "text") {
        for _ in text.chars() {
            if !semantic.question.budget.step() { return Known::Unknown }
        }
    }
    if let Some(Value::Record(flags)) = field(input, "flags") {
        for _ in flags.iter() {
            if !semantic.question.budget.step() { return Known::Unknown }
        }
    }
    let input = match Input::read(input) {
        Ok(input) => input,
        Err(message) => return Known::Yes(error("input", &message)),
    };
    let mapped = match ValueSource::new(input.text, source, input.extent, input.spans.as_deref()) {
        Ok(mapped) => mapped,
        Err(message) => return Known::Yes(error("input", &message)),
    };
    let context = match parser.context(&input.flags) {
        Ok(context) => context,
        Err(message) => return Known::Yes(error("input", &message)),
    };
    let parsed = match parser.parser.parse_root(input.text, context, &parser.registration) {
        Ok(parsed) => parsed,
        Err(failure) => {
            let offset = mapped.range(failure.offset, failure.offset).ok().map(|range| Value::Num(range.start as f64)).unwrap_or(Value::Null);
            return Known::Yes(Value::record(vec![
                ("ok".into(), Value::Bool(false)),
                ("root".into(), Value::Null),
                ("nodes".into(), Value::list(Vec::new())),
                ("error".into(), Value::record(vec![
                    ("kind".into(), Value::from("parse")),
                    ("start".into(), Value::Num(f64::from(failure.offset))),
                    ("offset".into(), offset),
                    ("expected".into(), Value::list(failure.expected.into_iter().map(Value::from).collect())),
                    ("committed".into(), Value::Bool(failure.committed)),
                ])),
            ]));
        }
    };
    let mut parents = vec![Value::Null; parsed.nodes.len()];
    for (id, node) in parsed.nodes.iter().enumerate() {
        for child in &node.children {
            parents[child.node as usize] = Value::Num(id as f64);
        }
    }
    let mut nodes = Vec::with_capacity(parsed.nodes.len());
    for (id, node) in parsed.nodes.iter().enumerate() {
        if !semantic.question.budget.step() { return Known::Unknown }
        let range = match mapped.range(node.start, node.end) {
            Ok(range) => range,
            Err(message) => return Known::Yes(error("mapping", &message)),
        };
        let schema = if node.matcher == crate::grammar::TEXT { None } else { parser.parser.nodes().get(node.matcher as usize) };
        let fields = schema.map_or_else(Vec::new, |schema| schema.fields.iter().enumerate().map(|(label, field)| {
            let ids = node.children.iter().filter(|edge| usize::from(edge.label) == label)
                .map(|edge| Value::Num(f64::from(edge.node))).collect();
            (field.name.clone().into(), Value::list(ids))
        }).collect());
        nodes.push(Value::record(vec![
            ("id".into(), Value::Num(id as f64)),
            ("parent".into(), parents[id].clone()),
            ("kind".into(), Value::text_copy(schema.map_or("TEXT", |schema| schema.name.as_str()))),
            ("children".into(), Value::list(node.children.iter().map(|edge| Value::Num(f64::from(edge.node))).collect())),
            ("fields".into(), Value::record(fields)),
            ("text".into(), Value::text_copy(&input.text[node.start as usize..node.end as usize])),
            ("start".into(), Value::Num(f64::from(node.start))),
            ("end".into(), Value::Num(f64::from(node.end))),
            ("offset".into(), Value::Num(range.start as f64)),
            ("end_offset".into(), Value::Num(range.end as f64)),
            ("raw".into(), Value::text_copy(&source[range])),
        ]));
    }
    Known::Yes(Value::record(vec![
        ("ok".into(), Value::Bool(true)),
        ("root".into(), Value::Num(f64::from(parsed.root))),
        ("nodes".into(), Value::list(nodes)),
        ("error".into(), Value::Null),
    ]))
}

struct Input<'a> {
    text: &'a str,
    extent: std::ops::Range<usize>,
    spans: Option<Vec<std::ops::Range<usize>>>,
    flags: Vec<(String, bool)>,
}

impl<'a> Input<'a> {
    fn read(value: &'a Value<'static>) -> Result<Self, String> {
        let Some(Value::Str(text)) = field(value, "text") else { return Err("mapped input text must be a string".into()) };
        let extent = span(value)?;
        let spans = match field(value, "spans") {
            None | Some(Value::Null) => None,
            Some(Value::List(spans)) => {
                if spans.len() != text.chars().count() {
                    return Err("mapped source needs one span per Unicode scalar".into());
                }
                Some(spans.iter().map(span).collect::<Result<Vec<_>, _>>()?)
            }
            _ => return Err("mapped input spans must be a list".into()),
        };
        let flags = match field(value, "flags") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Record(flags)) => flags.iter().map(|(name, value)| {
                let Value::Bool(enabled) = value else { return Err("mapped parser flags must be booleans".into()) };
                Ok((name.to_string(), *enabled))
            }).collect::<Result<Vec<_>, String>>()?,
            _ => return Err("mapped parser flags must be a record".into()),
        };
        Ok(Self { text, extent, spans, flags })
    }
}

fn field<'a>(record: &'a Value<'static>, name: &str) -> Option<&'a Value<'static>> {
    let Value::Record(fields) = record else { return None };
    fields.iter().find(|(key, _)| key.as_ref() == name).map(|(_, value)| value)
}

fn span(value: &Value<'static>) -> Result<std::ops::Range<usize>, String> {
    let offset = |name| match field(value, name) {
        Some(Value::Num(number)) if number.is_finite() && *number >= 0.0 && number.fract() == 0.0 && *number <= f64::from(u32::MAX) => Ok(*number as usize),
        _ => Err(format!("mapped `{name}` must be a nonnegative parser byte offset")),
    };
    Ok(offset("offset")?..offset("end_offset")?)
}

fn error(kind: &str, message: &str) -> Value<'static> {
    Value::record(vec![
        ("ok".into(), Value::Bool(false)),
        ("root".into(), Value::Null),
        ("nodes".into(), Value::list(Vec::new())),
        ("error".into(), Value::record(vec![
            ("kind".into(), Value::text_copy(kind)),
            ("message".into(), Value::text_copy(message)),
        ])),
    ])
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::{linter, lint};

    const PARSER: &str = r#"
        parser toy in t {
            token character /./
            line_break /[\r\n]/
            goal code { character }
            node root { item: text* }
            root entry = emit(root, item:character+ end)
        }
    "#;

    #[test]
    fn mapped_value_trees_drive_real_vm_fixes_at_the_written_escape() {
        let rules = format!(r#"
            {PARSER}
            in .t match literal /'[^']*'/
            in literal rule mapped {{
                let decoded js_string_parts(text)
                let input {{ text: decoded.text, spans: decoded.spans, offset: offset + 1, end_offset: end_offset - 1 }}
                let parsed syntax_tree(node, "toy", input)
                let parsed_nodes get(parsed, "nodes")
                let tree_root {{ value parsed_nodes; filter (entry) entry.id == parsed.root; first }}
                let first_child first(tree_root.children)
                each part in parsed_nodes
                when part.kind == "TEXT" && part.text == "a"
                when part.parent == parsed.root && part.id == first_child
                when tree_root.children == tree_root.fields.item && count(tree_root.children) == 2
                fix at {{ start: part.offset, end: part.end_offset }} as "A"
                warn "mapped value"
            }}
        "#);
        let linter = linter(&rules);
        let fixed = linter.fix(std::path::Path::new("/root/file.t"), r"'\x61b'");
        assert_eq!(fixed.applied, 1);
        assert_eq!(fixed.text, "'Ab'");
    }

    #[test]
    fn unknown_parsers_and_invalid_mappings_return_errors_not_partial_successes() {
        let rules = format!(r#"
            {PARSER}
            in .t match word /[a-z]+/
            in word rule errors {{
                let input {{ text: text, offset: offset, end_offset: end_offset, spans: [] }}
                let missing syntax_tree(node, "missing", input)
                let invalid syntax_tree(node, "toy", input)
                let missing_nodes get(missing, "nodes")
                let invalid_nodes get(invalid, "nodes")
                when missing.ok == false && invalid.ok == false
                when count(missing_nodes) == 0 && count(invalid_nodes) == 0
                warn "explicit errors"
            }}
        "#);
        assert_eq!(lint(&linter(&rules), "ab").len(), 1);
    }
}
