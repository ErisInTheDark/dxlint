//! Lexical environments retained for nested class summaries.

use super::{Builder, DeclKind, Scope, TypeSyn, type_syn};
use super::super::kinds::{Field as F, Kind as K};
use super::super::view::View;

impl<V: View> Builder<'_, V> {
    pub(super) fn lexical_ancestors(&mut self, ancestors: &[u32], ambient: bool) -> Option<u32> {
        let v = self.v;
        let mut parent = None;
        for (index, &node) in ancestors.iter().enumerate() {
            let outer = index.checked_sub(1).map(|index| ancestors[index]);
            let kind = v.kind(node);
            let child = ancestors.get(index + 1).copied();
            if matches!(kind, Some(K::Class | K::Method)) && child.is_some_and(|child|
                v.all(node, F::Decorator).contains(&child) || kind == Some(K::Method) && v.get(node, F::Key) == Some(child))
                || kind == Some(K::Switch) && child.is_some_and(|child| v.get(node, F::Discriminant) == Some(child)) { continue }
            let function = matches!(kind, Some(K::Function | K::Arrow | K::Method));
            let function_body = kind == Some(K::Block) && outer.is_some_and(|outer|
                matches!(v.kind(outer), Some(K::Function | K::Arrow | K::Method)) && v.get(outer, F::Body) == Some(node));
            if !function && !matches!(kind, Some(K::Block | K::StaticBlock | K::For | K::ForIn | K::ForOf | K::Switch | K::Class)) || function_body { continue }
            if let Some(existing) = self.out.decls.iter().position(|decl| decl.kind == DeclKind::Lexical && decl.start == v.start(node)) {
                parent = Some(existing as u32);
                continue;
            }
            let frame = self.out.decls.len() as u32;
            let mut decl = self.base(node, "<scope>", DeclKind::Lexical, ambient);
            decl.parent = parent;
            self.out.decls.push(decl);
            let first = self.out.decls.len();
            let mut scope = Scope::default();
            self.lexical = true;
            if function {
                for parameter in v.all(node, F::Param) {
                    if let Some(name) = v.get(parameter, F::Name) {
                        self.lexical_pattern(name, v.get(parameter, F::Type).map(|ty| type_syn(v, ty)), &mut scope, ambient);
                    }
                }
                if kind == Some(K::Function) && outer.is_some_and(|outer| !matches!(v.kind(outer), Some(K::Block | K::Program | K::Export))) {
                    self.statement(node, &mut scope, None, ambient);
                }
                if let Some(body) = v.get(node, F::Body) {
                    self.lexical_statements(body, &mut scope, ambient);
                    self.hoisted_variables(body, &mut scope, ambient);
                }
            } else {
                match kind {
                    Some(K::Block | K::Switch | K::StaticBlock) => {
                        let body = if kind == Some(K::StaticBlock) { v.get(node, F::Body).unwrap_or(node) } else { node };
                        self.lexical_statements(body, &mut scope, ambient);
                        if kind == Some(K::StaticBlock) { self.hoisted_variables(body, &mut scope, ambient) }
                        if let Some(outer) = outer.filter(|&outer| v.is(outer, K::Try) && v.get(outer, F::Handler) == Some(node)) {
                            if let Some(parameter) = v.get(outer, F::Param) {
                                self.lexical_pattern(parameter, v.get(outer, F::Type).map(|ty| type_syn(v, ty)), &mut scope, ambient);
                            }
                        }
                    }
                    Some(K::For | K::ForIn | K::ForOf) => {
                        for field in [F::Init, F::Left] {
                            if let Some(variable) = v.get(node, field).filter(|&variable| v.is(variable, K::Var) && !v.has_word(variable, F::Kind, "var")) {
                                self.statement(variable, &mut scope, None, ambient);
                            }
                        }
                    }
                    Some(K::Class) => {
                        if let Some(name) = v.get(node, F::Name) {
                            if let Some(class) = self.out.decls.iter().position(|decl| decl.kind == DeclKind::Class && decl.start == v.start(node)) {
                                scope.names.insert(v.text(name).into(), vec![class as u32]);
                            }
                        }
                    }
                    _ => {}
                }
            }
            self.lexical = false;
            for decl in &mut self.out.decls[first..] {
                if decl.parent.is_none() { decl.parent = Some(frame) }
            }
            self.out.decls[frame as usize].scope = Some(scope);
            parent = Some(frame);
        }
        parent
    }

    fn lexical_statements(&mut self, body: u32, scope: &mut Scope, ambient: bool) {
        let v = self.v;
        let statements = if v.is(body, K::Switch) {
            v.all(body, F::Case).into_iter().flat_map(|case| v.all(case, F::Body)).collect()
        } else { v.all(body, F::Body) };
        for statement in statements {
            if !v.is(statement, K::Var) || !v.has_word(statement, F::Kind, "var") {
                self.statement(statement, scope, None, ambient);
            }
        }
    }

    fn hoisted_variables(&mut self, node: u32, scope: &mut Scope, ambient: bool) {
        let v = self.v;
        let mut pending = vec![node];
        while let Some(node) = pending.pop() {
            if matches!(v.kind(node), Some(K::Function | K::Arrow | K::Method | K::Class | K::Namespace | K::StaticBlock)) { continue }
            if v.is(node, K::Var) && v.has_word(node, F::Kind, "var") {
                self.statement(node, scope, None, ambient);
            }
            pending.extend(v.children(node).rev());
        }
    }

    pub(super) fn lexical_pattern(&mut self, pattern: u32, ty: Option<TypeSyn>, scope: &mut Scope, ambient: bool) {
        let v = self.v;
        match v.kind(pattern) {
            Some(K::Ident) => {
                let mut decl = self.base(pattern, v.text(pattern), DeclKind::Var, ambient);
                decl.type_params.clear();
                decl.ty = ty;
                self.push(scope, decl);
            }
            Some(K::AssignPattern) => {
                if let Some(left) = v.get(pattern, F::Left) { self.lexical_pattern(left, ty, scope, ambient) }
            }
            Some(K::Rest) => {
                if let Some(argument) = v.get(pattern, F::Arg) { self.lexical_pattern(argument, None, scope, ambient) }
            }
            Some(K::ObjectPattern) => {
                for property in v.all(pattern, F::Property) {
                    if let Some(value) = v.get(property, F::Value).or_else(|| v.get(property, F::Key)) { self.lexical_pattern(value, None, scope, ambient) }
                }
                for rest in v.all(pattern, F::Rest) { self.lexical_pattern(rest, None, scope, ambient) }
            },
            Some(K::ArrayPattern) => for element in v.all(pattern, F::Element) { self.lexical_pattern(element, None, scope, ambient) },
            _ => {}
        }
    }
}
