//! Member groups, initializer dependencies and configured declaration order.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::syntax::Syntax;

struct Member {
    name: String,
    offset: usize,
    optional: bool,
    groups: Vec<String>,
    skip_rank: bool,
    initializer: bool,
    dependencies: Vec<String>,
}

fn member_kind(s: &Syntax, node: u32) -> Option<&'static str> {
    let readonly = s.has_word(node, F::Modifier, "readonly");
    match s.kind(node) {
        Some(K::Method | K::MethodSig) => Some(if s.is(node, K::Method)
            && s.parent(node).is_some_and(|p| s.is(p, K::Class)) && !s.has_word(node, F::Modifier, "static")
            && s.get(node, F::Key).is_some_and(|n| !s.is(n, K::Computed))
            && crate::native::ts::statics::property_name(s, node).as_deref() == Some("constructor")
        { "constructor" } else {
            match s.word(node, F::Kind) { Some("get") => "get", Some("set") => "set", _ => "method" }
        }),
        Some(K::CallSig) => Some(if s.word(node, F::Kind) == Some("new") { "constructor" } else { "call-signature" }),
        Some(K::PropSig) => Some(if readonly { "readonly-field" } else { "field" }),
        Some(K::ClassProp) => Some(if s.has_word(node, F::Modifier, "accessor") { "accessor" }
            else if s.get(node, F::Init).is_some_and(|n| matches!(s.kind(s.unparen(n)), Some(K::Arrow | K::Function))) { "method" }
            else if readonly { "readonly-field" } else { "field" }),
        Some(K::IndexSig) => Some(if readonly { "readonly-signature" } else { "signature" }),
        Some(K::StaticBlock) => Some("static-initialization"),
        _ => None,
    }
}

fn groups(s: &Syntax, node: u32, kind: &str, modifiers: bool) -> Vec<String> {
    let mut groups = Vec::new();
    let mut add = |prefix: &str, fallback: bool| {
        groups.push(format!("{prefix}{kind}"));
        if fallback && kind == "readonly-field" { groups.push(format!("{prefix}field")); }
    };
    if modifiers {
        let access = ["public", "protected", "private"].into_iter().find(|m| s.has_word(node, F::Modifier, m))
            .unwrap_or_else(|| if s.get(node, F::Key).is_some_and(|n| s.is(n, K::PrivateName)) { "#private" } else { "public" });
        let scope = if s.has_word(node, F::Modifier, "static") { "static" }
            else if s.has_word(node, F::Modifier, "abstract") { "abstract" } else { "instance" };
        if s.all(node, F::Decorator).next().is_some() && matches!(kind, "readonly-field" | "field" | "method" | "accessor" | "get" | "set") {
            add(&format!("{access}-decorated-"), false);
            add("decorated-", false);
            if kind == "readonly-field" {
                groups.push(format!("{access}-decorated-field"));
                groups.push("decorated-field".into());
            }
        }
        if !matches!(kind, "readonly-signature" | "signature" | "static-initialization") {
            if kind != "constructor" {
                groups.push(format!("{access}-{scope}-{kind}"));
                groups.push(format!("{scope}-{kind}"));
                if kind == "readonly-field" {
                    groups.push(format!("{access}-{scope}-field"));
                    groups.push(format!("{scope}-field"));
                }
            }
            groups.push(format!("{access}-{kind}"));
            if kind == "readonly-field" { groups.push(format!("{access}-field")); }
        }
    }
    groups.push(kind.into());
    if kind == "readonly-field" { groups.push("field".into()); }
    if kind == "readonly-signature" { groups.push("signature".into()); }
    groups
}

fn immediate_dependencies(file: &NativeFile, node: u32, names: &mut Vec<String>) {
    let s = &file.syntax;
    if matches!(s.kind(node), Some(K::Function | K::Arrow | K::Method | K::Class)) { return; }
    if matches!(s.kind(node), Some(K::Member | K::Index)) && s.get(node, F::Object).is_some_and(|n| s.is(n, K::This)) {
        let name = s.get(node, F::Property).map(|n| s.text(n).trim_start_matches('#').to_string())
            .or_else(|| s.get(node, F::Index).and_then(|n| file.static_string(n)));
        if let Some(name) = name { if !names.contains(&name) { names.push(name); } }
    }
    for (_, child) in s.kids(node) { immediate_dependencies(file, child, names); }
}

fn members(file: &NativeFile, node: u32) -> Vec<Member> {
    let s = &file.syntax;
    let class = s.is(node, K::Class);
    s.all(node, F::Member).map(|member| {
        let kind = member_kind(s, member).unwrap_or("");
        let name = match s.kind(member) {
            Some(K::CallSig) => if kind == "constructor" { "new".into() } else { "call".into() },
            Some(K::StaticBlock) => "static block".into(),
            Some(K::IndexSig) => s.all(member, F::Param).find_map(|p| s.get(p, F::Name).filter(|&n| s.is(n, K::Ident)).map(|n| s.text(n).to_string())).unwrap_or_else(|| "(index signature)".into()),
            _ if kind == "constructor" => "constructor".into(),
            _ => {
                let (name, shape) = super::class_styles::member_name(s, member);
                match shape {
                    "quoted" => name[1..name.len() - 1].to_string(),
                    "private" => name.trim_start_matches('#').to_string(),
                    _ => name,
                }
            }
        };
        let init = s.get(member, F::Init);
        let mut dependencies = Vec::new();
        if let Some(init) = init { immediate_dependencies(file, init, &mut dependencies); }
        Member {
            name,
            offset: s.tree.node(member).start as usize,
            optional: s.get(member, F::Optional).is_some(),
            groups: groups(s, member, kind, class),
            skip_rank: s.is(member, K::Method) && s.get(member, F::Body).is_none() && !s.has_word(member, F::Modifier, "abstract"),
            initializer: init.is_some(),
            dependencies,
        }
    }).collect()
}

fn standard_order() -> Vec<String> {
    let mut order = vec!["signature".into(), "call-signature".into()];
    for kind in ["field", "static-initialization", "constructor", "accessor", "get", "set", "method"] {
        if kind == "static-initialization" { order.push(kind.into()); continue; }
        if kind != "constructor" {
            for scope in ["static", "decorated", "instance", "abstract"] {
                for access in ["public", "protected", "private", "#private"] {
                    if scope == "abstract" && matches!(access, "private" | "#private") || scope == "decorated" && access == "#private" { continue; }
                    order.push(format!("{access}-{scope}-{kind}"));
                }
            }
        }
        for access in ["public", "protected", "private", "#private"] {
            if kind == "constructor" && access == "#private" { continue; }
            order.push(format!("{access}-{kind}"));
        }
        if kind != "constructor" {
            for scope in ["static", "instance", "abstract", "decorated"] { order.push(format!("{scope}-{kind}")); }
        }
        order.push(kind.into());
    }
    order
}

pub(super) fn member_order_info(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let expression = s.is(node, K::Class) && !s.parent(node).is_some_and(|p| matches!(s.kind(p), Some(K::Program | K::Block | K::Export | K::Namespace)));
    Value::Record(vec![
        ("class_expression".into(), Value::Bool(expression)),
        ("standard_order".into(), Value::List(standard_order().into_iter().map(Value::from).collect())),
    ])
}

fn rank(member: &Member, order: &[Vec<String>]) -> Option<usize> {
    if member.skip_rank { return None; }
    member.groups.iter().find_map(|group| order.iter().position(|types| types.contains(group)))
}

fn report(member: &Member, message: String, result: &mut Vec<Value<'static>>) {
    result.push(Value::Record(vec![
        ("offset".into(), Value::from(member.offset)),
        ("message".into(), Value::from(message)),
    ]));
}

fn group_sort<'a>(members: &'a [Member], order: &[Vec<String>], result: &mut Vec<Value<'static>>) -> Option<Vec<Vec<&'a Member>>> {
    let mut ranks: Vec<usize> = Vec::new();
    let mut groups: Vec<Vec<&Member>> = Vec::new();
    let mut correct = true;
    for member in members {
        let Some(rank) = rank(member, order) else { continue; };
        match ranks.last().copied() {
            Some(last) if rank < last => {
                let lowest = *ranks.iter().filter(|&&r| r > rank).min().expect("a lower rank follows at least one higher rank");
                let label = order[lowest].iter().map(|s| s.replace('-', " ")).collect::<Vec<_>>().join(", ");
                report(member, format!("Member {} should be declared before all {label} definitions.", member.name), result);
                correct = false;
            }
            Some(last) if rank == last => groups.last_mut().expect("an equal rank has an existing group").push(member),
            _ => {
                ranks.push(rank);
                groups.push(vec![member]);
            }
        }
    }
    correct.then_some(groups)
}

fn fallback_groups<'a>(members: &'a [Member], order: &[Vec<String>]) -> Vec<Vec<&'a Member>> {
    let ranks: Vec<_> = members.iter().map(|m| rank(m, order)).collect();
    let mut previous = None;
    let mut groups: Vec<Vec<&Member>> = Vec::new();
    // Upstream's fallback only collects members before the final position.
    for i in 0..members.len().saturating_sub(1) {
        if previous == Some(ranks[i]) {
            groups.last_mut().expect("a previous rank has an existing group").push(&members[i]);
        } else if ranks[i] == ranks[i + 1] {
            groups.push(vec![&members[i]]);
            previous = Some(ranks[i]);
        }
    }
    groups
}

fn alpha_sort(members: &[&Member], order: &str, result: &mut Vec<Value<'static>>) {
    let insensitive = order.ends_with("case-insensitive");
    let natural = order.starts_with("natural");
    let mut previous_name = "";
    let mut previous_index = 0;
    for (index, &member) in members.iter().enumerate() {
        if member.name.is_empty() { continue; }
        let comparison = crate::native::ts::statics::compare_names(&member.name, previous_name, insensitive, natural);
        let incorrect = member.name != previous_name && if natural { !comparison.is_gt() } else { comparison.is_lt() };
        if incorrect {
            let blocked = member.initializer && members[previous_index..index].iter()
                .any(|earlier| earlier.initializer && member.dependencies.contains(&earlier.name));
            if blocked { continue; }
            report(member, format!("Member {} should be declared before member {previous_name}.", member.name), result);
        }
        previous_name = &member.name;
        previous_index = index;
    }
}

fn check_order(members: &[Member], full: &[Member], types: Option<&[Vec<String>]>, order: &str, result: &mut Vec<Value<'static>>) {
    let alpha = matches!(order, "alphabetically" | "alphabetically-case-insensitive" | "natural" | "natural-case-insensitive");
    if let Some(types) = types {
        let groups = group_sort(members, types, result);
        if alpha {
            for group in groups.unwrap_or_else(|| fallback_groups(full, types)) { alpha_sort(&group, order, result); }
        }
    } else if alpha {
        alpha_sort(&members.iter().collect::<Vec<_>>(), order, result);
    }
}

pub(super) fn member_order(file: &NativeFile, node: u32) -> Value<'static> {
    let args = crate::expr::facts::args();
    let types = args.first().and_then(|value| {
        if let Value::List(types) = value {
            Some(types.iter().map(|group| group.items().into_iter().map(|v| v.text().into_owned()).collect::<Vec<_>>()).collect::<Vec<_>>())
        } else { None }
    });
    let order = args.get(1).map(|v| v.text().into_owned()).unwrap_or_default();
    let optionality = args.get(2).map(|v| v.text().into_owned()).unwrap_or_default();
    let members = members(file, node);
    let mut result = Vec::new();
    let split = members.windows(2).position(|pair| pair[0].optional != pair[1].optional).map(|i| i + 1);
    if let Some(split) = split.filter(|_| matches!(optionality.as_str(), "optional-first" | "required-first")) {
        let label = if optionality == "optional-first" { "optional" } else { "required" };
        let bad = if members[0].optional != (optionality == "optional-first") { Some(&members[0]) }
            else if members[split + 1..].iter().any(|m| m.optional != members[split].optional) { Some(&members[split]) }
            else { None };
        if let Some(member) = bad {
            report(member, format!("Member {} should be declared after all {label} members.", member.name), &mut result);
        } else {
            check_order(&members[..split], &members, types.as_deref(), &order, &mut result);
            check_order(&members[split..], &members, types.as_deref(), &order, &mut result);
        }
    } else {
        check_order(&members, &members, types.as_deref(), &order, &mut result);
    }
    Value::List(result)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn member_groups_follow_configured_specificity() {
        let linter = native_linter("rule member-ordering");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "class A { method() {} field = 1; }").len(), 1);
        let linter = native_linter(r#"rule member-ordering { set classes ["method", "field"] }"#);
        assert!(linter.lint(Path::new("/root/file.ts"), "class A { method() {} field = 1; }").is_empty());
    }

    #[test]
    fn default_object_options_retain_groups_but_class_options_do_not() {
        let linter = native_linter(r#"rule member-ordering { set default { order: "alphabetically" } }"#);
        assert!(linter.lint(Path::new("/root/file.ts"), "class A { z = 1; a() {} }").is_empty());
        let linter = native_linter(r#"rule member-ordering { set classes { order: "alphabetically" } }"#);
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "class A { z = 1; a() {} }").len(), 1);
    }

    #[test]
    fn alphabetic_sorting_preserves_immediate_initializer_dependencies() {
        let linter = native_linter(r#"rule member-ordering { set classes { memberTypes: "never", order: "alphabetically" } }"#);
        assert!(linter.lint(Path::new("/root/file.ts"), "class A { z = 1; a = this.z; b = this.a; }").is_empty());
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "class A { z = 1; a = () => this.z; }").len(), 1);
    }
}
