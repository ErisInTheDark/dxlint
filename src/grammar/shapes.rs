//! Possible field results and physical tree relations, computed once per grammar.

use super::{Grammar, ROOT, TEXT};
use std::collections::HashMap;
use crate::pattern::{compile::NO_LABEL, Pat};

#[derive(Clone, Debug, Default)]
pub struct FieldTargets {
    /// Grammar matcher ids; TEXT is represented separately by `text`.
    pub kinds: Vec<u32>,
    pub text: bool,
    pub any: bool,
}

#[derive(Default)]
pub(super) struct Shapes {
    fields: HashMap<(u32, u16), FieldTargets>,
    children: HashMap<u32, Vec<u32>>,
    parents: HashMap<u32, Vec<u32>>,
}

impl Shapes {
    pub(super) fn build(grammar: &Grammar) -> Self {
        let mut shapes = Self::default();
        for (id, matcher) in grammar.matchers.iter().enumerate() {
            let id = id as u32;
            let mut body = if let Some(native) = grammar.native_kind(id) {
                let mut body = Body::default();
                if native.front == 0 {
                    use crate::native::ts::kinds::{field_shape, ALL_FIELDS, ALL_KINDS};
                    let front = &grammar.natives[native.front as usize];
                    for &field in ALL_FIELDS {
                        let shape = field_shape(ALL_KINDS[native.kind as usize], field);
                        let targets = FieldTargets {
                            kinds: shape.kinds.iter().flat_map(|kind| variants(grammar, front.base + *kind as u32)).collect(),
                            text: shape.text,
                            any: shape.any,
                        };
                        body.field(front.labels[field as usize], targets);
                    }
                } else {
                    for label in matcher.labels() {
                        if let Some(label) = grammar.labels.get(&label) {
                            body.field(label, FieldTargets { any: true, ..FieldTargets::default() });
                        }
                    }
                }
                body
            } else {
                pattern(grammar, &matcher.pat, &matcher.children, &mut shapes)
            };
            // Skip consumes attached children inside its capture context.
            // Without Skip, fill_children appends them to the matcher itself.
            if matcher.native.is_some() || !matcher.has_skip {
                body.nodes(NO_LABEL, matcher.children.iter().copied());
            }
            body.nodes(NO_LABEL, matcher.overlays.iter().copied().filter(|&id| grammar.matchers[id as usize].layer.is_none()));
            if matcher.native.is_some_and(|native| native.front == 0 && native.kind == crate::native::ts::kinds::Kind::Program as u16) {
                for language in &grammar.langs {
                    body.nodes(NO_LABEL, language.children.iter().copied());
                    body.nodes(NO_LABEL, language.overlays.iter().copied().filter(|&id| grammar.matchers[id as usize].layer.is_none()));
                }
                if !grammar.layers.is_empty() {
                    body.nodes(NO_LABEL, [ROOT]);
                }
            }
            shapes.merge(id, body);
        }
        let mut root = Body::default();
        for language in &grammar.langs {
            root.nodes(NO_LABEL, language.children.iter().copied());
            root.nodes(NO_LABEL, language.overlays.iter().copied().filter(|&id| grammar.matchers[id as usize].layer.is_none()));
        }
        for (id, matcher) in grammar.matchers.iter().enumerate() {
            if matcher.layer.is_some() {
                root.nodes(NO_LABEL, [id as u32]);
            }
        }
        if !grammar.layers.is_empty() {
            root.nodes(NO_LABEL, [ROOT]);
        }
        shapes.merge(ROOT, root);
        for (&parent, children) in &shapes.children {
            for &child in children {
                shapes.parents.entry(child).or_default().push(parent);
            }
        }
        for ids in shapes.children.values_mut().chain(shapes.parents.values_mut()) {
            ids.sort_unstable();
            ids.dedup();
        }
        for targets in shapes.fields.values_mut() {
            targets.kinds.sort_unstable();
            targets.kinds.dedup();
        }
        shapes
    }

    fn merge(&mut self, matcher: u32, body: Body) {
        self.children.entry(matcher).or_default().extend(body.children);
        for (label, targets) in body.fields {
            self.fields.entry((matcher, label)).or_default().merge(targets);
        }
    }
}

impl FieldTargets {
    fn merge(&mut self, other: Self) {
        self.kinds.extend(other.kinds);
        self.text |= other.text;
        self.any |= other.any;
    }
}

#[derive(Default)]
struct Body {
    fields: HashMap<u16, FieldTargets>,
    children: Vec<u32>,
}

impl Body {
    fn field(&mut self, label: u16, targets: FieldTargets) {
        if targets.kinds.is_empty() && !targets.text && !targets.any {
            return;
        }
        self.children.extend(&targets.kinds);
        if targets.text {
            self.children.push(TEXT);
        }
        self.fields.entry(label).or_default().merge(targets);
    }

    fn nodes(&mut self, label: u16, kinds: impl IntoIterator<Item = u32>) {
        self.field(label, FieldTargets { kinds: kinds.into_iter().collect(), ..FieldTargets::default() });
    }

    fn merge(&mut self, other: Self) {
        self.children.extend(other.children);
        for (label, targets) in other.fields {
            self.fields.entry(label).or_default().merge(targets);
        }
    }
}

fn variants(grammar: &Grammar, base: u32) -> impl Iterator<Item = u32> + '_ {
    (0..grammar.matchers.len() as u32).filter(move |&id| grammar.is_a(id, base))
}

fn pattern(grammar: &Grammar, pat: &Pat, children: &[u32], shapes: &mut Shapes) -> Body {
    let mut body = Body::default();
    match pat {
        Pat::Seq(items) | Pat::Alt(items) => {
            for item in items {
                body.merge(pattern(grammar, item, children, shapes));
            }
        }
        Pat::Repeat { inner, max, .. } if *max != Some(0) => return pattern(grammar, inner, children, shapes),
        Pat::Ref { name, label } => {
            if let Some(target) = grammar.matcher(name) {
                body.nodes(label.as_deref().and_then(|label| grammar.labels.get(label)).unwrap_or(NO_LABEL), variants(grammar, target));
            }
        }
        Pat::Capture { name, inner } => {
            let inner = pattern(grammar, inner, children, shapes);
            let label = grammar.labels.get(name).expect("compiled capture label");
            body.field(label, FieldTargets { text: true, ..FieldTargets::default() });
            // Tree::fields sees other labels through TEXT, but a same-label
            // outer capture stops traversal. Physical children stay under TEXT.
            for (&inner_label, targets) in &inner.fields {
                if inner_label != label {
                    body.fields.entry(inner_label).or_default().merge(targets.clone());
                }
            }
            // TEXT has no distinct matcher per capture context; union those
            // contexts here, rather than inventing runtime matcher identities.
            shapes.merge(TEXT, inner);
        }
        Pat::Skip => body.nodes(NO_LABEL, children.iter().copied()),
        // Assertions emit no tree edges, even when their regex has captures.
        _ => {}
    }
    body
}

impl Grammar {
    /// Possible results of Tree::fields, including fields exposed through
    /// TEXT captures. Unknown matcher/label pairs have no targets.
    pub fn field_targets(&self, matcher: u32, label: u16) -> &FieldTargets {
        static EMPTY: FieldTargets = FieldTargets { kinds: Vec::new(), text: false, any: false };
        self.shapes.fields.get(&(matcher, label)).unwrap_or(&EMPTY)
    }

    /// Physical child matcher ids, including TEXT and ROOT wrappers.
    pub fn children_kinds(&self, matcher: u32) -> &[u32] {
        self.shapes.children.get(&matcher).map_or(&[], Vec::as_slice)
    }

    /// Inverse physical relation; unlike Tree::owner, does not skip TEXT.
    pub fn parent_kinds(&self, matcher: u32) -> &[u32] {
        self.shapes.parents.get(&matcher).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::source::Disk;
    use crate::grammar::Tree;

    fn grammar(source: &str) -> Grammar {
        let loaded = crate::config::load_text("shapes.zlint", source, &Disk);
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        let mut errors = Vec::new();
        let grammar = Grammar::build(&loaded.config, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        grammar
    }

    fn validate(grammar: &Grammar, tree: &Tree) {
        for &id in &tree.order {
            let parent = tree.node(id).matcher;
            for edge in &tree.node(id).children {
                let child = tree.node(edge.node).matcher;
                assert!(grammar.children_kinds(parent).contains(&child), "{parent}: child {child}");
                assert!(grammar.parent_kinds(child).contains(&parent), "{child}: parent {parent}");
                let shape = grammar.field_targets(parent, edge.label);
                assert!(shape.any || if child == TEXT { shape.text } else { shape.kinds.contains(&child) }, "{parent}.{}: child {child}", edge.label);
            }
            for label in grammar.matchers.get(parent as usize).map(|m| m.labels()).unwrap_or_default() {
                let label = grammar.labels.get(&label).unwrap();
                let shape = grammar.field_targets(parent, label);
                for child in tree.fields(id, label) {
                    let child = tree.node(child).matcher;
                    assert!(shape.any || if child == TEXT { shape.text } else { shape.kinds.contains(&child) }, "{parent}.{label}: projected child {child}");
                }
            }
        }
    }

    #[test]
    fn pattern_fields_and_relations_follow_effective_trees() {
        let grammar = grammar(r#"
lang t "*.t"
match ident /[a-z]+/
match number /\d+/
in .t match pair sig:(value:ident ":" value:number) or sig:(value:number ":" value:ident)
in pair match punctuation /:/
in pair layer marks match mark /[a-z]+/
in .t match list contents:("[" until "]")
in list match ident
"#);
        for source in ["abc:12", "12:abc", "[abc def]"] {
            validate(&grammar, &Tree::parse(&grammar, 0, source));
        }
    }

    #[test]
    fn captures_shadow_only_their_own_label_and_refs_keep_field_ownership() {
        let grammar = grammar(r#"
lang t "*.t"
match ident /[a-z]+/
match opaque nested:ident
in .t match shadow value:(value:ident ":" other:ident)
in .t match boundary "[" opaque "]"
in .t match repeated "(" (item:ident or item:/\d+/)+ ")"
"#);
        for source in ["abc:def", "[abc]", "(abc12def)"] {
            validate(&grammar, &Tree::parse(&grammar, 0, source));
        }
        let shadow = grammar.matcher("shadow").unwrap();
        let value = grammar.labels.get("value").unwrap();
        let targets = grammar.field_targets(shadow, value);
        assert!(targets.text);
        assert!(targets.kinds.is_empty(), "outer capture must hide same-label inner node");
        assert_eq!(grammar.field_targets(shadow, grammar.labels.get("other").unwrap()).kinds, [grammar.matcher("ident").unwrap()]);
        assert!(grammar.field_targets(grammar.matcher("boundary").unwrap(), grammar.labels.get("nested").unwrap()).kinds.is_empty());
        let repeated = grammar.field_targets(grammar.matcher("repeated").unwrap(), grammar.labels.get("item").unwrap());
        assert!(repeated.text && repeated.kinds.contains(&grammar.matcher("ident").unwrap()));
    }

    #[test]
    fn native_fields_survive_tokens_specialisations_and_pattern_grafts() {
        let grammar = grammar(r#"
import .ts
in ts_identifier match specialised like ts_identifier /x/
in ts_block match comment /\/\/[^\r\n]*/
in ts_call layer calls match call_text /f/
"#);
        for source in [
            "function f(x: number) { // note\n return x + 1; } f(2);",
            "class C<T> { x?: T; m(y: T): T { return y; } }",
            "const f = <T,>(x:T) => <div key={x}>{x}</div>;",
        ] {
            let mut parsed = crate::native::ts::parse(source, true).unwrap();
            crate::native::ts::tokens::attach_selected(source, &mut parsed, &crate::native::ts::tokens::ALL_TOKENS);
            let front = &grammar.natives[0];
            let nodes = front.remap(parsed.nodes);
            validate(&grammar, &Tree::from_native(nodes.clone(), parsed.root));
            validate(&grammar, &Tree::graft(&grammar, grammar.lang("js").unwrap(), source, nodes, parsed.root, None, None));
        }
        let specialised = grammar.matcher("specialised").unwrap();
        let name = grammar.labels.get("name").unwrap();
        let parameter = grammar.matcher("ts_parameter").unwrap();
        assert!(grammar.field_targets(parameter, name).kinds.contains(&specialised));
        println!("native grammar: {} matchers, {} fields, {} any", grammar.matchers.len(), grammar.shapes.fields.len(), grammar.shapes.fields.values().filter(|shape| shape.any).count());
    }

    #[test]
    fn built_in_pattern_languages_follow_shapes() {
        for (language, source) in [
            ("json", r#"{"compilerOptions":{"strict":true},"items":[1,null,"x"]}"#),
            ("css", "a:hover { color: red; width: calc(100% - 1px); }"),
            ("python", "def f(x):\n    # note\n    return [x, \"value\"]\n"),
            ("markdown", "# Heading\n\n[text](url) and `code`\n\n```ts\nconst x = 1;\n```\n"),
        ] {
            let grammar = grammar(&format!("import .{language}\n"));
            validate(&grammar, &Tree::parse(&grammar, grammar.lang(language).unwrap(), source));
            println!("{language}: {} matchers, {} fields, {} any", grammar.matchers.len(), grammar.shapes.fields.len(), grammar.shapes.fields.values().filter(|shape| shape.any).count());
        }
    }
}
