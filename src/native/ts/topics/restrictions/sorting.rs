//! Ordering decisions use string order, never the expression engine's numeric coercion.

use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::statics::{compare_names as compare, property_name};
use crate::expr::Value;
use std::cmp::Ordering;

pub fn variables(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let names: Vec<_> = s.all(node, F::Decl).filter_map(|n| {
        let id = s.get(n, F::Name).filter(|&id| s.is(id, K::Ident))?;
        Some((n, s.text(id)))
    }).collect();
    let mut results = Vec::new();
    for insensitive in [false, true] {
        let mut greatest = "";
        for (i, &(n, name)) in names.iter().enumerate() {
            if i > 0 && compare(name, greatest, insensitive, false) == Ordering::Less {
                results.push(Value::Record(vec![
                    ("ignoreCase".into(), Value::Bool(insensitive)),
                    ("offset".into(), Value::from(s.tree.node(n).start as usize)),
                ]));
            } else {
                greatest = name;
            }
        }
    }
    Value::List(results)
}

pub fn keys(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let properties: Vec<_> = s.all(node, F::Property).collect();
    let mut results = Vec::new();
    for ignore_computed in [false, true] {
        for line_groups in [false, true] {
            let mut previous_name: Option<String> = None;
            let mut previous_node = None;
            let mut previous_blank = false;
            for &n in &properties {
                if s.is(n, K::Spread) {
                    previous_name = None;
                    continue;
                }
                let key = s.get(n, F::Key).unwrap_or(n);
                if ignore_computed && s.is(key, K::Computed) {
                    previous_name = None;
                    continue;
                }
                let expression = s.get(key, F::Expr).unwrap_or(key);
                let name = property_name(s, n).or_else(|| s.is(expression, K::Ident).then(|| s.text(expression).to_string()));
                let blank = previous_blank || previous_node.is_some_and(|p| {
                    let from = s.tree.node(p).end as usize;
                    let to = s.tree.node(n).start as usize;
                    let gap = &file.syntax.src[from..to];
                    gap.find('\n').zip(gap.rfind('\n')).is_some_and(|(first, last)| first < last && gap[first + 1..last].split('\n').any(|line| line.trim().is_empty()))
                });
                let prev = previous_name.clone();
                previous_node = Some(n);
                if name.is_some() { previous_name = name.clone(); }
                if line_groups && blank {
                    previous_blank = name.is_none();
                    continue;
                }
                let (Some(prev), Some(name)) = (prev, name) else { continue };
                for insensitive in [false, true] {
                    for natural in [false, true] {
                        let order = compare(&prev, &name, insensitive, natural);
                        if order == Ordering::Equal { continue; }
                        results.push(Value::Record(vec![
                            ("ignoreComputedKeys".into(), Value::Bool(ignore_computed)),
                            ("allowLineSeparatedGroups".into(), Value::Bool(line_groups)),
                            ("caseSensitive".into(), Value::Bool(!insensitive)),
                            ("natural".into(), Value::Bool(natural)),
                            ("mode".into(), Value::from(if order == Ordering::Less { "desc" } else { "asc" })),
                            ("count".into(), Value::from(properties.len())),
                            ("name".into(), Value::from(name.clone())),
                            ("previous".into(), Value::from(prev.clone())),
                            ("offset".into(), Value::from(s.tree.node(key).start as usize)),
                        ]));
                    }
                }
            }
        }
    }
    Value::List(results)
}
