//! Compiled attribute dispatch and reads through the file's existing memo.

use super::{CExpr, Ctx, Value};
use crate::grammar::{Grammar, ROOT, TEXT};

/// A `let` attribute: its expression on each layer that defines it.
pub struct Attr {
    pub name: String,
    /// Specificity order, resolved by the config compiler.
    pub defs: Vec<(u32, CExpr)>,
}

pub struct Attributes {
    attrs: Vec<CompiledAttr>,
}

struct CompiledAttr {
    name: String,
    defs: Vec<CExpr>,
    routes: Vec<Route>,
}

#[derive(Clone, Copy)]
enum Route {
    Missing,
    Defined(u32),
    Forward(u16),
}

impl Attributes {
    pub fn new(attrs: Vec<Attr>, forwards: &[(u32, u16, u16)], grammar: &Grammar) -> Self {
        if attrs.is_empty() { return Self { attrs: Vec::new() }; }
        let ancestors: Vec<Vec<usize>> = (0..grammar.matchers.len()).map(|kind| {
            let mut chain = Vec::new();
            let mut at = Some(kind as u32);
            while let Some(id) = at {
                chain.push(id as usize);
                at = grammar.matchers[id as usize].like;
            }
            chain
        }).collect();
        let mut by_attr = vec![Vec::new(); attrs.len()];
        for (index, &(matcher, attr, label)) in forwards.iter().enumerate() {
            by_attr[attr as usize].push((matcher, index, label));
        }
        let attrs = attrs.into_iter().zip(by_attr).map(|(attr, forwards)| {
            let mut routes = Vec::new();
            if !attr.defs.is_empty() || !forwards.is_empty() {
                let mut definitions = vec![u32::MAX; grammar.matchers.len()];
                for (index, (matcher, _)) in attr.defs.iter().enumerate() {
                    definitions[*matcher as usize] = definitions[*matcher as usize].min(index as u32);
                }
                let mut forwarding = vec![None; grammar.matchers.len()];
                for (matcher, index, label) in forwards {
                    forwarding[matcher as usize] = Some((index, label));
                }
                routes = ancestors.iter().map(|chain| {
                    let definition = chain.iter().map(|&id| definitions[id]).min().unwrap_or(u32::MAX);
                    if definition != u32::MAX { return Route::Defined(definition); }
                    // Forwards keep declaration order, not nearest-ancestor order.
                    chain.iter().filter_map(|&id| forwarding[id]).min_by_key(|(index, _)| *index)
                        .map_or(Route::Missing, |(_, label)| Route::Forward(label))
                }).collect();
            }
            CompiledAttr {
                name: attr.name,
                defs: attr.defs.into_iter().map(|(_, expr)| expr).collect(),
                routes,
            }
        }).collect();
        Self { attrs }
    }

    fn route(&self, matcher: u32, attr: u16) -> Route {
        if matcher == ROOT || matcher == TEXT { return Route::Missing; }
        self.attrs[attr as usize].routes.get(matcher as usize).copied().unwrap_or(Route::Missing)
    }
}

impl<'a> Ctx<'a> {
    pub(super) fn attr(&self, node: u32, attr: u16) -> Result<Value<'a>, String> {
        if self.memo.params.borrow().len() == self.memo.param_base.get() {
            self.attr_unbound(node, attr)
        } else {
            self.unbound(|| self.attr_unbound(node, attr))
        }
    }

    fn attr_unbound(&self, node: u32, attr: u16) -> Result<Value<'a>, String> {
        let index = match self.attrs.route(self.tree.node(node).matcher, attr) {
            Route::Missing => return Ok(Value::Null),
            Route::Forward(label) => {
                let mut values = Vec::new();
                for id in self.tree.fields(node, label) {
                    values.push(self.attr(id, attr)?);
                }
                return Ok(match values.len() {
                    0 => Value::Null,
                    1 => values.pop().expect("one"),
                    _ => Value::list(values),
                });
            }
            Route::Defined(index) => index as usize,
        };
        if let Some(v) = self.memo.values.borrow().get(&(node, attr)) {
            return Ok(v.clone());
        }
        let def = &self.attrs.attrs[attr as usize];
        if self.memo.busy.borrow().contains(&(node, attr)) {
            return Err(format!("attribute `{}` depends on itself", def.name));
        }
        self.memo.busy.borrow_mut().push((node, attr));
        let ctx = Ctx { node, options: &[], ..*self };
        let value = ctx.eval(&def.defs[index]);
        self.memo.busy.borrow_mut().pop();
        let value = value?;
        self.memo.values.borrow_mut().insert((node, attr), value.clone());
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{self, source::Disk};

    fn grammar() -> Grammar {
        let loaded = config::load_text("attrs.zlint", r#"
            lang t "*.t"
            match base "a"
            match member like base "b"
            match leaf like member "c"
            match other "d"
            in .t {
                match leaf
                match other
            }
        "#, &Disk);
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        let mut errors = Vec::new();
        let grammar = Grammar::build(&loaded.config, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        grammar
    }

    fn number(value: f64) -> CExpr { CExpr::Const(Value::Num(value)) }

    #[test]
    fn definitions_keep_compiler_priority_over_inherited_forwards() {
        let g = grammar();
        let base = g.matcher("base").unwrap();
        let member = g.matcher("member").unwrap();
        let leaf = g.matcher("leaf").unwrap();
        let other = g.matcher("other").unwrap();
        let plan = Attributes::new(vec![
            Attr { name: "specific".into(), defs: vec![(member, number(2.0)), (base, number(1.0))] },
            Attr { name: "fallback".into(), defs: vec![(base, number(3.0))] },
        ], &[(leaf, 0, 0), (leaf, 1, 0)], &g);
        let read = |attr| match plan.route(leaf, attr) {
            Route::Defined(index) => match &plan.attrs[attr as usize].defs[index as usize] {
                CExpr::Const(Value::Num(n)) => *n,
                _ => panic!("number"),
            },
            _ => panic!("definition"),
        };
        assert_eq!(read(0), 2.0);
        assert_eq!(read(1), 3.0);
        for matcher in [other, ROOT, TEXT] {
            assert!(matches!(plan.route(matcher, 0), Route::Missing));
        }
    }

    #[test]
    fn forwards_keep_first_matching_declaration_not_nearest_ancestor() {
        let g = grammar();
        let base = g.matcher("base").unwrap();
        let member = g.matcher("member").unwrap();
        let leaf = g.matcher("leaf").unwrap();
        let plan = Attributes::new(vec![
            Attr { name: "broad_first".into(), defs: Vec::new() },
            Attr { name: "narrow_first".into(), defs: Vec::new() },
        ], &[(base, 0, 7), (member, 0, 9), (member, 1, 9), (base, 1, 7)], &g);
        assert!(matches!(plan.route(leaf, 0), Route::Forward(7)));
        assert!(matches!(plan.route(leaf, 1), Route::Forward(9)));
        for matcher in [ROOT, TEXT] {
            assert!(matches!(plan.route(matcher, 0), Route::Missing));
        }
    }

    #[test]
    fn forwarded_values_keep_empty_scalar_and_list_cardinality_across_files() {
        use std::path::Path;
        use crate::engine::tests::native_linter;

        let lint = native_linter(r#"
            in ts_identifier let size len(text)
            in ts_call forward size from argument
            in ts_parenthesized forward size from expression
            in ts_call rule sizes info "{count([] size)}:{sum([] size)}"
        "#);
        for (source, expected) in [
            ("f(); f(a); f(a, bc);", vec!["0:0", "1:1", "2:3"]),
            ("f((long));", vec!["1:4"]),
        ] {
            let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
            assert_eq!(diagnostics.iter().map(|d| d.message.as_str()).collect::<Vec<_>>(), expected);
        }
        let defined = native_linter(r#"
            in ts_identifier let size len(text)
            in ts_call let size 7
            in ts_call forward size from argument
            in ts_call rule sizes info "{size}"
        "#);
        assert_eq!(defined.lint(Path::new("/root/a.ts"), "f(a, bc);")[0].message, "7");
    }

    #[test]
    fn inherited_definitions_refine_defaults_and_cross_node_reads() {
        use std::path::Path;
        use crate::engine::tests::native_linter;

        let lint = native_linter(r#"
            in ts_node let size 1
            in ts_identifier let size len(text)
            in ts_binary rule operands info "{left.size}:{right.size}"
            in ts_identifier rule direct info "{size}"
        "#);
        let messages = |source| {
            let mut values: Vec<_> = lint.lint(Path::new("/root/a.ts"), source).into_iter().map(|d| d.message).collect();
            values.sort();
            values
        };
        assert_eq!(messages("a + long;"), ["1", "1:4", "4"]);
        assert_eq!(messages("other + b;"), ["1", "5", "5:1"]);
    }

    #[test]
    fn failed_and_cyclic_attributes_leave_other_reads_and_later_files_usable() {
        use std::path::Path;
        use crate::config::ast::Severity;
        use crate::engine::tests::native_linter;

        let lint = native_linter(r#"
            in ts_identifier let bad byte_slice(text, [])
            in ts_identifier let cyclic cyclic
            in ts_identifier let size len(text)
            in ts_identifier rule first info "{bad}"
            in ts_identifier rule second info "{bad}"
            in ts_identifier rule cycle info "{cyclic}"
            in ts_identifier rule good info "{size}"
        "#);
        for (source, size) in [("a;", "1"), ("long;", "4")] {
            let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
            assert_eq!(diagnostics.iter().filter(|d| d.severity == Severity::Error).count(), 3);
            for rule in ["first", "second"] {
                let failure = diagnostics.iter().find(|d| d.rule == rule).unwrap();
                assert!(!failure.message.contains("depends on itself"), "{failure:?}");
            }
            assert!(diagnostics.iter().find(|d| d.rule == "cycle").unwrap().message.contains("depends on itself"));
            assert_eq!(diagnostics.iter().find(|d| d.rule == "good").unwrap().message, size);
        }
    }
}
