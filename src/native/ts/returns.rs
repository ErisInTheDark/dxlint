//! Return types of unannotated functions declared in the file being
//! linted, from their `return` statements. Other files stay annotation-only
//! so their summaries stay small.

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};
use super::summary::Sig;
use super::types::Ty;

impl NativeFile<'_> {
    /// Return/yield sites in this function, excluding nested functions/classes.
    pub fn return_sites(&self, func: u32) -> impl Iterator<Item = u32> + '_ {
        let s = &self.syntax;
        let mut stack: Vec<_> = s.get(func, F::Body).into_iter().collect();
        std::iter::from_fn(move || {
            while let Some(node) = stack.pop() {
                match s.kind(node) {
                    Some(K::Function | K::Arrow | K::Method | K::Class) => {}
                    Some(K::Return | K::Yield) => return Some(node),
                    _ => stack.extend(s.kids(node).map(|(_, child)| child)),
                }
            }
            None
        })
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
