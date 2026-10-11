//! Return types of unannotated functions declared in the file being
//! linted, from their `return` statements. Other files stay annotation-only
//! so their summaries stay small.

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};
use super::summary::Sig;
use super::types::Ty;

impl NativeFile<'_> {
    /// Return/yield sites in this function or program, excluding nested scopes.
    pub fn return_sites(&self, func: u32) -> impl Iterator<Item = u32> + '_ {
        let s = &self.syntax;
        let mut stack: Vec<_> = if s.is(func, K::Program) { vec![func] } else { s.get(func, F::Body).into_iter().collect() };
        std::iter::from_fn(move || {
            while let Some(node) = stack.pop() {
                match s.kind(node) {
                    Some(K::Function | K::Arrow | K::Method | K::Class | K::StaticBlock) => {}
                    Some(K::Return | K::Yield) => return Some(node),
                    _ => stack.extend(s.kids(node).map(|(_, child)| child)),
                }
            }
            None
        })
    }

    pub fn own_returns(&self, func: u32) -> Vec<u32> {
        let mut returns: Vec<_> = self.return_sites(func).filter(|&node| self.syntax.is(node, K::Return)).collect();
        returns.sort_by_key(|&node| self.syntax.tree.node(node).start);
        returns
    }

    /// What calling `sig` returns, when it is this file's own unannotated
    /// function (`None` otherwise).
    pub(super) fn own_return(&self, sig: &Sig, file: super::modules::FileId) -> Option<Ty> {
        let typing = self.typing();
        if sig.ret.is_some() || file != typing.checker.file {
            return None;
        }
        let func = self.function_at(sig.body_at?)?;
        if let Some(t) = typing.returns.borrow().get(&func) {
            return Some(t.clone());
        }
        // Recursion sees unknown.
        typing.returns.borrow_mut().insert(func, Ty::Unknown);
        let t = self.body_return(func, sig.is_async);
        typing.returns.borrow_mut().insert(func, t.clone());
        Some(t)
    }

    /// The function-like node starting at `at`.
    fn function_at(&self, at: u32) -> Option<u32> {
        let typing = self.typing();
        let map = typing.functions.get_or_init(|| {
            let s = &self.syntax;
            let mut map = std::collections::HashMap::new();
            let mut stack = vec![s.root()];
            while let Some(n) = stack.pop() {
                if matches!(s.kind(n), Some(K::Function | K::Method | K::Arrow)) {
                    map.entry(s.tree.node(n).start).or_insert(n);
                }
                stack.extend(s.kids(n).map(|(_, c)| c));
            }
            map
        });
        map.get(&at).copied()
    }

    /// The union of the function's `return`s (`void` without any);
    /// Generators also retain their yielded values.
    fn body_return(&self, func: u32, is_async: bool) -> Ty {
        let s = &self.syntax;
        let ck = &self.typing().checker;
        let generator = s.has_word(func, F::Modifier, "*");
        let Some(body) = s.get(func, F::Body) else { return Ty::Unknown };
        let mut yielded = Vec::new();
        let t = if !s.is(body, K::Block) {
            self.type_of(body)
        } else {
            let mut found = Vec::new();
            for n in self.return_sites(func) {
                match s.kind(n) {
                    Some(K::Return) => found.push(s.get(n, F::Arg).map_or(Ty::Undefined, |a| self.type_of(a).widen())),
                    Some(K::Yield) if generator => {
                        if self.tokens(n).get(1) == Some(&"*") {
                            yielded.push(Ty::Unknown);
                        } else {
                            yielded.push(s.get(n, F::Arg).map_or(Ty::Undefined, |a| self.type_of(a).widen()));
                        }
                    }
                    _ => {}
                }
            }
            if found.iter().any(Ty::is_unknown) {
                return Ty::Unknown;
            }
            if !found.is_empty() && self.reach().0.falls_off.get(&func).copied().unwrap_or(false) {
                found.push(Ty::Undefined);
            }
            if found.is_empty() {
                Ty::Void
            } else {
                Ty::union(found)
            }
        };
        if generator {
            let yielded = Ty::union(yielded);
            return ck.global_named(if is_async { "AsyncGenerator" } else { "Generator" }, vec![if is_async { ck.awaited(&yielded) } else { yielded }, t, Ty::Unknown]);
        }
        if is_async {
            ck.global_named("Promise", vec![ck.awaited(&t)])
        } else {
            t
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::with_native_file;
    use crate::native::ts::kinds::{Field as F, Kind as K};

    #[test]
    fn own_returns_keep_source_order_and_scope_boundaries() {
        with_native_file("function outer(x: boolean) { function nested() { return 1; } const arrow = () => { return 2; }; class C { method() { return 3; } static { } } if (x) return; return void nested(); } return 4;", |file, _| {
            let program = (0..file.syntax.tree.nodes.len() as u32).find(|&node| file.syntax.is(node, K::Program)).unwrap();
            let outer = (0..file.syntax.tree.nodes.len() as u32).find(|&node| file.syntax.is(node, K::Function) && file.syntax.get(node, F::Name).is_some_and(|name| file.syntax.text(name) == "outer")).unwrap();
            let returns = file.own_returns(outer);
            assert_eq!(returns.len(), 2);
            assert!(file.syntax.get(returns[0], F::Arg).is_none());
            assert!(file.syntax.get(returns[1], F::Arg).is_some());
            let program_returns = file.own_returns(program);
            assert_eq!(program_returns.len(), 1);
            assert_eq!(file.syntax.get(program_returns[0], F::Arg).map(|argument| file.static_number(argument)), Some(Some(4.0)));
        });
    }
}
