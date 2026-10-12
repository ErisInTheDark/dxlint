use super::super::question::{Known, NodeRef, ScopeRef, Target};
use super::super::semantic::Semantic;

pub struct Queries<'a> {
    #[expect(dead_code, reason = "queries are stubs until the publication adapter lands")]
    pub(crate) semantic: &'a Semantic<'a>,
}

impl Queries<'_> {
    pub fn definition(&self, _name: NodeRef, _class: u64) -> Known<Option<Vec<Target>>> {
        Known::Unknown
    }

    pub fn namespace_members(&self, _scope: ScopeRef, _key: &str, _class: u64) -> Known<Option<Vec<Target>>> {
        Known::Unknown
    }

    pub fn global(&self, _importer: u32, _name: &str, _class: u64) -> Known<Vec<Target>> {
        Known::Unknown
    }
}

#[cfg(any())]
mod tests {
    use super::*;
    use super::super::compile::{Action, Conditional};
    use super::super::super::question::{Leaf, QueryContext, World};
    use super::super::super::{KindNames, Names, Resolved, ScopeIndex, ScopeInfo};
    use crate::config::ast::Config;
    use crate::grammar::{Edge, Grammar, Node, Tree};

    struct Files {
        grammar: Grammar,
        trees: Vec<Tree>,
        names: Vec<Resolved>,
    }

    impl Files {
        fn new() -> Self {
            let trees = (0..2).map(|_| Tree::from_native(vec![
                Node { matcher: 0, start: 0, end: 3, children: vec![Edge { label: 0, node: 1 }, Edge { label: 0, node: 2 }] },
                Node { matcher: 1, start: 0, end: 1, children: Vec::new() },
                Node { matcher: 2, start: 2, end: 3, children: Vec::new() },
            ], 0)).collect();
            let names = (0..2).map(|_| {
                let mut names = Resolved::default();
                names.scopes.push(ScopeInfo { node: 0, classes: 1, parent: None });
                names.declared.insert(1, (0, 1, 1, true));
                names.declared.insert(2, (0, 2, 1, true));
                names
            }).collect();
            Self { grammar: Grammar::build(&Config::default(), &mut Vec::new()), trees, names }
        }
    }

    impl World for Files {
        fn grammar(&self) -> &Grammar { &self.grammar }
        fn tree(&self, file: u32) -> Option<&Tree> { self.trees.get(file as usize) }
        fn text(&self, _: u32) -> Option<&str> { Some("X P") }
        fn names(&self, file: u32) -> Option<&Resolved> { self.names.get(file as usize) }
        fn index(&self, file: u32) -> Option<&ScopeIndex> {
            Some(self.names(file)?.index(self.tree(file)?, self.text(file)?, &|text| text.into()))
        }
        fn name_key<'s>(&self, _: u32, text: &'s str) -> std::borrow::Cow<'s, str> { text.into() }
        fn module(&self, _: Leaf, _: &str, _: &Semantic<'_>) -> Known<Option<ScopeRef>> { Known::Yes(Some(ScopeRef { file: 1, scope: 0 })) }
        fn global_scopes(&self, _: u32, _: &Semantic<'_>) -> Known<Vec<ScopeRef>> {
            Known::Yes(vec![ScopeRef { file: 0, scope: 0 }, ScopeRef { file: 1, scope: 0 }])
        }
        fn condition(&self, _: Leaf, _: u32, _: &Semantic<'_>) -> Known<bool> { Known::Yes(true) }
        fn operand(&self, at: Leaf, operand: u32, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> {
            let Target::Node(node) = at.target else { return Known::Unknown };
            if operand == 1 { return Known::Yes(crate::expr::Value::Str("other".to_string().into())) }
            if operand == 2 { return Known::Yes(crate::expr::Value::Str("X".to_string().into())) }
            Known::Yes(crate::expr::Value::Node(node.node, "X".to_string().into()))
        }
        fn attribute(&self, _: Leaf, _: u16, _: &Semantic<'_>) -> Known<crate::expr::Value<'static>> { Known::Yes(crate::expr::Value::Null) }
    }

    fn tables(first: bool) -> Names {
        let mut kinds = vec![KindNames::default(); 3];
        kinds[0].modules.items.push(Conditional { action: Action::Uses("test".into()), when: None });
        kinds[1].modules.items.extend([
            Conditional { action: Action::Exports { classes: 1, asks: 0, target: Some(0), module: None, alias: None }, when: None },
            Conditional { action: Action::Globals { first, classes: 1, asks: 0, target: Some(0), module: None }, when: None },
        ]);
        Names { kinds, ..Names::default() }
    }

    fn target(file: u32) -> Target { Target::Node(NodeRef { file, node: 1 }) }

    #[test]
    fn publication_scopes_do_not_expose_private_lexical_declarations() {
        let files = Files::new();
        let tables = tables(false);
        let question = QueryContext::default();
        let semantic = Semantic::new(&files, &tables, &question);
        let scope = ScopeRef { file: 0, scope: 0 };
        assert_eq!(semantic.modules().namespace_members(scope, "X", 1), Known::Yes(Some(vec![target(0)])));
        assert_eq!(semantic.modules().namespace_members(scope, "P", 1), Known::Yes(Some(Vec::new())));
    }

    #[test]
    fn ordinary_globals_merge_but_first_globals_choose_one_real_declaration() {
        let files = Files::new();
        for (first, expected) in [(false, vec![target(0), target(1)]), (true, vec![target(0)])] {
            let tables = tables(first);
            let question = QueryContext::default();
            let semantic = Semantic::new(&files, &tables, &question);
            assert_eq!(semantic.modules().global(0, "X", 1), Known::Yes(expected));
            assert_eq!(semantic.modules().global(0, "Missing", 1), Known::Yes(Vec::new()));
        }
    }

    #[test]
    fn import_alias_definition_accepts_uses_and_declarations_without_exposing_source_classes() {
        let mut files = Files::new();
        files.trees[0].nodes[1].matcher = 3;
        files.names[0].declared.get_mut(&1).unwrap().2 = 2;
        files.names[0].declaration.insert(2, 1);
        let mut tables = tables(false);
        let mut import = KindNames::default();
        import.modules.items.push(Conditional {
            action: Action::Resolves { classes: 2, asks: 1, binding: 0, module: 1, imported: Some(2) },
            when: None,
        });
        tables.kinds.push(import);
        let question = QueryContext::default();
        let semantic = Semantic::new(&files, &tables, &question);
        for node in [1, 2] {
            assert_eq!(semantic.modules().definition(NodeRef { file: 0, node }, 2), Known::Yes(Some(vec![target(1)])));
            assert_eq!(semantic.modules().definition(NodeRef { file: 0, node }, 1), Known::Yes(Some(Vec::new())));
        }
        assert_eq!(semantic.modules().definition(NodeRef { file: 1, node: 1 }, 1), Known::Yes(None));
    }
}
