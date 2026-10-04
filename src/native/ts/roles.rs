//! What a function is for, as eslint's accessor rules see it: getters and
//! setters of classes and object literals, and the `get` / `set` functions
//! of property descriptors (`Object.defineProperty(o, "x", { get() {} })`).

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

    /// A key of the options object of a dynamic `import(x, { ... })`:
    /// `Some(None)` for its own keys (`with`), `Some(Some(name))` for keys
    /// of an object one level in, under property `name` (`type` in
    /// `{ with: { type: "json" } }`).
    pub fn import_options_key(&self, node: u32) -> Option<Option<String>> {
        let s = &self.syntax;
        if s.field(node) != Some(F::Key) {
            return None;
        }
        let object = s.parent(node).filter(|&n| s.is(n, K::Prop)).and_then(|p| s.parent(p)).filter(|&n| s.is(n, K::Object))?;
        let holder = s.parent(object)?;
        if s.is(holder, K::ImportCall) {
            return Some(None);
        }
        let outer = s.parent(holder).filter(|&n| s.is(holder, K::Prop) && s.is(n, K::Object))?;
        s.parent(outer).filter(|&n| s.is(n, K::ImportCall))?;
        Some(super::statics::property_name(s, holder))
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

    /// `all`, `some` of the function's own `return`s give a value; `bare`
    /// when there are only `return;`s, `none` without any.
    pub fn return_values(&self, func: u32) -> Option<&'static str> {
        let s = &self.syntax;
        let body = s.get(func, F::Body)?;
        let (mut with, mut without) = (0, 0);
        let mut stack = vec![body];
        while let Some(n) = stack.pop() {
            match s.kind(n) {
                Some(K::Function | K::Arrow | K::Method | K::Class) => continue,
                Some(K::Return) => {
                    if s.get(n, F::Arg).is_some() {
                        with += 1;
                    } else {
                        without += 1;
                    }
                }
                _ => stack.extend(s.kids(n).map(|(_, c)| c)),
            }
        }
        Some(match (with, without) {
            (0, 0) => "none",
            (0, _) => "bare",
            (_, 0) => "all",
            _ => "some",
        })
    }

    /// The `if`, loop or `?:` whose test `node` is part of (not across a
    /// function).
    pub fn conditional_ancestor(&self, node: u32) -> Option<u32> {
        let s = &self.syntax;
        let mut at = node;
        loop {
            let parent = s.parent(at)?;
            if matches!(s.kind(parent), Some(K::If | K::While | K::DoWhile | K::For | K::Conditional)) && s.field(at) == Some(F::Test) {
                return Some(parent);
            }
            if matches!(s.kind(parent), Some(K::Function | K::Arrow | K::Method)) {
                return None;
            }
            at = parent;
        }
    }

    /// eslint's no-unsafe-finally: a `return`, `throw`, `break` or
    /// `continue` that leaves a `finally` block (rather than a loop,
    /// switch or label inside it).
    pub fn leaves_finally(&self, node: u32) -> bool {
        let s = &self.syntax;
        let kind = s.kind(node);
        let label = s.get(node, F::Label).map(|l| s.text(l));
        let mut child = node;
        while let Some(parent) = s.parent(child) {
            match s.kind(parent) {
                Some(K::Function | K::Arrow | K::Method | K::Class | K::StaticBlock | K::ClassProp) => return false,
                Some(K::Try) if s.get(parent, F::Finalizer) == Some(child) => return true,
                Some(K::Labeled) if label.is_some() && s.get(parent, F::Label).map(|l| s.text(l)) == label => return false,
                Some(K::For | K::ForIn | K::ForOf | K::While | K::DoWhile) if label.is_none() && matches!(kind, Some(K::Break | K::Continue)) => return false,
                Some(K::Switch) if label.is_none() && kind == Some(K::Break) => return false,
                _ => {}
            }
            child = parent;
        }
        false
    }

    /// A `yield` inside `node`, not in a nested function (for a `for` loop,
    /// not in its initializer: that runs once, before the loop).
    pub fn contains_yield(&self, node: u32) -> bool {
        let s = &self.syntax;
        let mut stack: Vec<u32> = if s.is(node, K::For) { s.kids(node).filter(|(f, _)| *f != Some(F::Init)).map(|(_, c)| c).collect() } else { vec![node] };
        while let Some(n) = stack.pop() {
            match s.kind(n) {
                Some(K::Function | K::Arrow | K::Method | K::Class) if n != node => {}
                Some(K::Yield) => return true,
                _ => stack.extend(s.kids(n).map(|(_, c)| c)),
            }
        }
        false
    }

    /// Names its parameters declare more than once (`function f(a, a)`).
    pub fn duplicate_parameters(&self, func: u32) -> Vec<String> {
        let s = &self.syntax;
        let mut names: Vec<&str> = Vec::new();
        let mut stack: Vec<u32> = s.all(func, F::Param).filter_map(|p| s.get(p, F::Name)).collect();
        stack.reverse();
        while let Some(n) = stack.pop() {
            let mut kids: Vec<u32> = match s.kind(n) {
                Some(K::Ident) => {
                    names.push(s.text(n));
                    continue;
                }
                Some(K::ObjectPattern) => s
                    .all(n, F::Property)
                    .filter_map(|p| s.get(p, F::Value).filter(|v| !(s.is(*v, K::AssignPattern) && s.get(*v, F::Left).is_none())).or_else(|| s.get(p, F::Key)))
                    .chain(s.get(n, F::Rest))
                    .collect(),
                Some(K::ArrayPattern) => s.all(n, F::Element).collect(),
                Some(K::AssignPattern) => s.get(n, F::Left).into_iter().collect(),
                Some(K::Rest) => s.get(n, F::Arg).into_iter().collect(),
                _ => continue,
            };
            kids.reverse();
            stack.extend(kids);
        }
        let mut dup: Vec<String> = Vec::new();
        for (i, n) in names.iter().enumerate() {
            if names[..i].contains(n) && !dup.iter().any(|d| d == n) {
                dup.push(n.to_string());
            }
        }
        dup
    }

    /// `get` or `set` when the function is an accessor or a property
    /// descriptor's `get` / `set`.
    pub fn accessor(&self, func: u32) -> Option<&'static str> {
        let s = &self.syntax;
        let word = |w: &str| match w {
            "get" => Some("get"),
            "set" => Some("set"),
            _ => None,
        };
        // `get x() {}` in a class or object literal.
        if s.is(func, K::Method) {
            if let Some(k) = s.word(func, F::Kind).and_then(word) {
                return Some(k);
            }
        }
        // `{ get: function () {} }`, `{ get() {} }` in a descriptor.
        let (holder, name) = match s.kind(func)? {
            K::Method => (func, super::statics::property_name(s, func)?),
            K::Function | K::Arrow => {
                let prop = s.parent(func).filter(|&p| s.is(p, K::Prop) && s.field(func) == Some(F::Value))?;
                (prop, super::statics::property_name(s, prop)?)
            }
            _ => return None,
        };
        let kind = word(&name)?;
        let object = s.parent(holder).filter(|&o| s.is(o, K::Object))?;
        self.is_property_descriptor(object).then_some(kind)
    }

    /// `Object.defineProperty(o, k, {...})`, `Reflect.defineProperty(o, k,
    /// {...})`, or a value of `Object.create(p, { k: {...} })` /
    /// `Object.defineProperties(o, { k: {...} })`.
    fn is_property_descriptor(&self, object: u32) -> bool {
        let s = &self.syntax;
        if self.argument_of_global_method(object, &[("Object", "defineProperty", 2), ("Reflect", "defineProperty", 2)]) {
            return true;
        }
        let Some(prop) = s.parent(object).filter(|&p| s.is(p, K::Prop) && s.field(object) == Some(F::Value)) else { return false };
        let Some(outer) = s.parent(prop).filter(|&o| s.is(o, K::Object)) else { return false };
        self.argument_of_global_method(outer, &[("Object", "create", 1), ("Object", "defineProperties", 1)])
    }

    /// `node` is argument `index` of `Global.method(...)` (also `?.` and
    /// in parentheses), with `Global` the real global.
    fn argument_of_global_method(&self, node: u32, methods: &[(&str, &str, usize)]) -> bool {
        let s = &self.syntax;
        let Some(call) = s.parent(node).filter(|&c| s.is(c, K::Call) && s.field(node) == Some(F::Arg)) else { return false };
        let index = s.all(call, F::Arg).position(|a| a == node);
        let mut callee = s.get(call, F::Callee);
        while let Some(c) = callee.filter(|&c| s.is(c, K::Paren)) {
            callee = s.get(c, F::Expr);
        }
        let Some(callee) = callee.filter(|&c| matches!(s.kind(c), Some(K::Member | K::Index))) else { return false };
        let Some(object) = s.get(callee, F::Object).filter(|&o| s.is(o, K::Ident)) else { return false };
        let Some(name) = super::statics::property_name(s, callee) else { return false };
        let global = self.global_setting(object).is_some_and(|g| g != "off");
        global && methods.iter().any(|&(g, m, i)| s.text(object) == g && name == m && index == Some(i))
    }
}
