//! Function names, enclosing functions and duplicate parameters.

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};

impl NativeFile<'_> {
    /// The nearest function, arrow or method around `node` (not `node`
    /// itself).
    pub fn enclosing_function(&self, node: u32) -> Option<u32> {
        let s = &self.syntax;
        let mut at = s.parent(node);
        while let Some(n) = at {
            if matches!(s.kind(n), Some(K::Function | K::Arrow | K::Method)) {
                return Some(n);
            }
            at = s.parent(n);
        }
        None
    }

    /// eslint's `getFunctionNameWithKind`: `function 'f'`, `arrow function`,
    /// `static async getter 'x'`, `private method #y`, `constructor`.
    pub fn function_name_with_kind(&self, func: u32) -> Option<String> {
        let s = &self.syntax;
        let kind = s.kind(func)?;
        if !matches!(kind, K::Function | K::Arrow | K::Method) {
            return None;
        }
        let parent = s.parent(func);
        // What holds it: the method itself, an object property, a class field.
        let holder = match kind {
            K::Method => Some(func),
            _ => parent.filter(|&p| matches!(s.kind(p), Some(K::Prop | K::ClassProp)) && matches!(s.field(func), Some(F::Value | F::Init))),
        };
        let in_class = holder.is_some_and(|h| s.is(h, K::ClassProp) || s.parent(h).is_some_and(|p| s.is(p, K::Class)));
        let key = holder.and_then(|h| s.get(h, F::Key));
        let private = key.is_some_and(|k| s.is(k, K::PrivateName));
        let mut tokens: Vec<String> = Vec::new();
        if in_class {
            if holder.is_some_and(|h| s.has_word(h, F::Modifier, "static")) {
                tokens.push("static".into());
            }
            if private {
                tokens.push("private".into());
            }
        }
        if s.has_word(func, F::Modifier, "async") {
            tokens.push("async".into());
        }
        let generator = s.has_word(func, F::Modifier, "*") || (kind == K::Function && s.text(func).trim_start_matches("async").trim_start().trim_start_matches("function").trim_start().starts_with('*'));
        if generator {
            tokens.push("generator".into());
        }
        match holder {
            Some(h) if s.is(h, K::Method) => {
                if s.get(h, F::Key).is_some_and(|k| s.text(k) == "constructor") && in_class {
                    return Some("constructor".into());
                }
                tokens.push(
                    match s.word(h, F::Kind) {
                        Some("get") => "getter",
                        Some("set") => "setter",
                        _ => "method",
                    }
                    .into(),
                );
            }
            Some(_) => tokens.push("method".into()),
            None => {
                if kind == K::Arrow {
                    tokens.push("arrow".into());
                }
                tokens.push("function".into());
            }
        }
        let own_name = || s.get(func, F::Name).filter(|&n| kind == K::Function && s.is(n, K::Ident)).map(|n| format!("'{}'", super::statics::identifier_name(s.text(n))));
        match holder {
            Some(h) if private => tokens.push(s.text(s.get(h, F::Key)?).to_string()),
            Some(h) => {
                if let Some(name) = super::statics::property_name(s, h).map(|n| format!("'{n}'")).or_else(own_name) {
                    tokens.push(name);
                }
            }
            None => tokens.extend(own_name()),
        }
        Some(tokens.join(" "))
    }

    /// Where eslint reports a function: from its start (its property's, for
    /// methods and property values) or, for arrows, at the `=>`.
    pub fn function_head(&self, func: u32) -> Option<usize> {
        let s = &self.syntax;
        let holder = s.parent(func).filter(|&p| matches!(s.kind(p), Some(K::Prop | K::ClassProp)) && matches!(s.field(func), Some(F::Value | F::Init)));
        let node = match s.kind(func)? {
            K::Arrow if holder.is_none() => return s.get(func, F::Op).map(|op| s.tree.node(op).start as usize),
            K::Function | K::Arrow => holder.unwrap_or(func),
            K::Method => func,
            _ => return None,
        };
        Some(s.tree.node(node).start as usize)
    }
}
