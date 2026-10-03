//! Which global an expression stands for, following what eslint's
//! `ReferenceTracker` follows: the global itself (`Math`), through the
//! global object (`globalThis.Math`), and through variables that only ever
//! hold it (`const m = Math`).

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};

/// Names of the global object.
const GLOBAL_OBJECTS: &[&str] = &["globalThis", "global", "self", "window"];

impl NativeFile<'_> {
    /// An optional chain's outermost node (eslint's `ChainExpression`): in a
    /// chain with a `?.`, and not continued by its parent.
    fn chain_top(&self, node: u32) -> bool {
        let s = &self.syntax;
        let in_chain = |mut n: u32| loop {
            match s.kind(n) {
                Some(K::Member | K::Index) if s.get(n, F::Optional).is_some() => return true,
                Some(K::Call) if s.get(n, F::Optional).is_some() => return true,
                Some(K::Member | K::Index) => n = match s.get(n, F::Object) { Some(o) => o, None => return false },
                Some(K::Call) => n = match s.get(n, F::Callee) { Some(c) => c, None => return false },
                Some(K::NonNull) => n = match s.get(n, F::Expr) { Some(e) => e, None => return false },
                _ => return false,
            }
        };
        if !matches!(s.kind(node), Some(K::Member | K::Index | K::Call)) || !in_chain(node) {
            return false;
        }
        let continued = s.parent(node).is_some_and(|p| matches!(s.kind(p), Some(K::Member | K::Index | K::Call | K::NonNull)) && matches!(s.field(node), Some(F::Object | F::Callee | F::Expr)));
        !continued
    }

    /// eslint's `checkUndefinedShortCircuit`: the optional chains whose
    /// `undefined` `node` may evaluate to.
    fn short_circuits(&self, node: u32, out: &mut Vec<u32>) {
        let s = &self.syntax;
        let node = s.unparen(node);
        match s.kind(node) {
            Some(K::Logical) => match s.word(node, F::Op) {
                Some("||" | "??") => {
                    if let Some(r) = s.get(node, F::Right) {
                        self.short_circuits(r, out);
                    }
                }
                Some("&&") => {
                    for f in [F::Left, F::Right] {
                        if let Some(x) = s.get(node, f) {
                            self.short_circuits(x, out);
                        }
                    }
                }
                _ => {}
            },
            Some(K::Sequence) => {
                if let Some(l) = s.all(node, F::Expr).last() {
                    self.short_circuits(l, out);
                }
            }
            Some(K::Conditional) => {
                for f in [F::Consequent, F::Alternate] {
                    if let Some(x) = s.get(node, f) {
                        self.short_circuits(x, out);
                    }
                }
            }
            Some(K::Await) => {
                if let Some(a) = s.get(node, F::Arg) {
                    self.short_circuits(a, out);
                }
            }
            _ if self.chain_top(node) => out.push(node),
            _ => {}
        }
    }

    /// eslint's no-unsafe-optional-chaining at `node`: the optional chains
    /// that would make it throw (`false`) or compute `NaN` (`true`).
    pub fn optional_chain_risks(&self, node: u32) -> Vec<(u32, bool)> {
        let s = &self.syntax;
        let mut usage = Vec::new();
        let mut arithmetic = Vec::new();
        let destructuring = |n: u32| matches!(s.kind(s.unparen(n)), Some(K::Array | K::Object | K::ArrayPattern | K::ObjectPattern));
        let field = |f: F| s.get(node, f);
        match s.kind(node) {
            Some(K::Assign) => {
                if s.word(node, F::Op) == Some("=") && field(F::Left).is_some_and(destructuring) {
                    usage.extend(field(F::Right));
                }
                if matches!(s.word(node, F::Op), Some("+=" | "-=" | "/=" | "*=" | "%=" | "**=")) {
                    arithmetic.extend(field(F::Right));
                }
            }
            Some(K::AssignPattern) if field(F::Left).is_some_and(destructuring) => usage.extend(field(F::Right)),
            Some(K::Declarator) if field(F::Name).is_some_and(destructuring) => usage.extend(field(F::Init)),
            Some(K::Class) => usage.extend(field(F::Extends)),
            Some(K::Call) if field(F::Optional).is_none() => usage.extend(field(F::Callee)),
            Some(K::New) => usage.extend(field(F::Callee)),
            Some(K::Member | K::Index) if field(F::Optional).is_none() => usage.extend(field(F::Object)),
            Some(K::TaggedTemplate) => usage.extend(field(F::Tag)),
            Some(K::ForOf) => usage.extend(field(F::Right)),
            Some(K::Spread) if !s.parent(node).is_some_and(|p| s.is(p, K::Object)) => usage.extend(field(F::Arg)),
            Some(K::Binary) => {
                let op = s.word(node, F::Op).unwrap_or("");
                if op == "in" || op == "instanceof" {
                    usage.extend(field(F::Right));
                }
                if matches!(op, "+" | "-" | "/" | "*" | "%" | "**") {
                    arithmetic.extend(field(F::Right));
                    arithmetic.extend(field(F::Left));
                }
            }
            Some(K::With) => usage.extend(field(F::Object)),
            Some(K::Unary) if matches!(s.word(node, F::Op), Some("+" | "-")) => arithmetic.extend(field(F::Arg)),
            _ => {}
        }
        let mut out = Vec::new();
        for (list, arith) in [(usage, false), (arithmetic, true)] {
            for n in list {
                let mut found = Vec::new();
                self.short_circuits(n, &mut found);
                out.extend(found.into_iter().map(|c| (c, arith)));
            }
        }
        out
    }

    /// The string `node` always evaluates to, as eslint's
    /// `getStringIfConstant` finds it: literals, static templates, `+` of
    /// those, and variables that only ever hold one.
    pub fn static_string(&self, node: u32) -> Option<String> {
        self.static_string_at(node, 0)
    }

    fn static_string_at(&self, node: u32, depth: u32) -> Option<String> {
        if depth > 16 {
            return None;
        }
        let s = &self.syntax;
        match s.kind(node)? {
            // A regex is an object, not its text.
            K::Regex => None,
            K::Paren | K::As | K::Satisfies | K::NonNull => self.static_string_at(s.get(node, F::Expr)?, depth + 1),
            // `${...}` of static values.
            K::Template if s.all(node, F::Expr).next().is_some() => {
                let mut out = String::new();
                for (f, c) in s.kids(node).collect::<Vec<_>>() {
                    match f {
                        Some(F::Quasi) => {
                            let piece = s.text(c).trim_start_matches(['`', '}']).trim_end_matches('`');
                            let piece = piece.strip_suffix("${").unwrap_or(piece);
                            out.push_str(&super::regex::cook(&format!("`{piece}`"))?);
                        }
                        Some(F::Expr) => out.push_str(&self.static_string_at(c, depth + 1)?),
                        _ => {}
                    }
                }
                Some(out)
            }
            K::Binary if s.word(node, F::Op) == Some("+") => {
                let l = self.static_string_at(s.get(node, F::Left)?, depth + 1)?;
                let r = self.static_string_at(s.get(node, F::Right)?, depth + 1)?;
                Some(l + &r)
            }
            K::Ident => {
                let decl = *self.bindings().refs.get(&node)?;
                let b = self.binding(decl)?;
                if b.writes > 0 || !matches!(b.kind, "const" | "let" | "var") {
                    return None;
                }
                let declarator = s.parent(decl).filter(|&d| s.is(d, K::Declarator) && s.field(decl) == Some(F::Name))?;
                self.static_string_at(s.get(declarator, F::Init)?, depth + 1)
            }
            _ => super::statics::literal_text(s, node),
        }
    }

    /// The number (or bigint, or boolean as a number) `node` always
    /// evaluates to, as eslint's `getStaticValue` finds it.
    pub fn static_number(&self, node: u32) -> Option<f64> {
        self.static_number_at(node, 0)
    }

    fn static_number_at(&self, node: u32, depth: u32) -> Option<f64> {
        if depth > 16 {
            return None;
        }
        let s = &self.syntax;
        let text = s.text(node);
        match s.kind(node)? {
            K::Number => super::statics::number(text),
            K::BigInt => super::statics::number(text.trim_end_matches('n')),
            K::Bool => Some(if text == "true" { 1.0 } else { 0.0 }),
            K::Paren => self.static_number_at(s.get(node, F::Expr)?, depth + 1),
            K::Unary => {
                let v = self.static_number_at(s.get(node, F::Arg)?, depth + 1)?;
                match s.word(node, F::Op)? {
                    "-" => Some(-v),
                    "+" => Some(v),
                    "~" => Some(!(v as i64 as i32) as f64),
                    _ => None,
                }
            }
            K::Binary => {
                let l = self.static_number_at(s.get(node, F::Left)?, depth + 1)?;
                let r = self.static_number_at(s.get(node, F::Right)?, depth + 1)?;
                Some(match s.word(node, F::Op)? {
                    "+" => l + r,
                    "-" => l - r,
                    "*" => l * r,
                    "/" => l / r,
                    "%" => l % r,
                    "**" => l.powf(r),
                    _ => return None,
                })
            }
            K::Ident => {
                let decl = *self.bindings().refs.get(&node)?;
                let b = self.binding(decl)?;
                if b.writes > 0 || !matches!(b.kind, "const" | "let" | "var") {
                    return None;
                }
                let declarator = s.parent(decl).filter(|&d| s.is(d, K::Declarator) && s.field(decl) == Some(F::Name))?;
                self.static_number_at(s.get(declarator, F::Init)?, depth + 1)
            }
            _ => None,
        }
    }

    /// The global `node` evaluates to (`Math`), when it is one.
    pub fn global_value(&self, node: u32) -> Option<String> {
        self.global_value_at(node, 0)
    }

    fn global_value_at(&self, node: u32, depth: u32) -> Option<String> {
        if depth > 16 {
            return None;
        }
        let s = &self.syntax;
        match s.kind(node)? {
            K::Paren | K::NonNull | K::As | K::Satisfies | K::TypeAssertion => self.global_value_at(s.get(node, F::Expr)?, depth + 1),
            K::Ident => {
                if self.global_setting(node).is_some_and(|g| g != "off") {
                    return Some(s.text(node).to_string());
                }
                // A variable that only ever holds a global.
                let decl = *self.bindings().refs.get(&node)?;
                let b = self.binding(decl)?;
                if b.writes > 0 || !matches!(b.kind, "const" | "let" | "var") {
                    return None;
                }
                let declarator = s.parent(decl).filter(|&d| s.is(d, K::Declarator) && s.field(decl) == Some(F::Name))?;
                self.global_value_at(s.get(declarator, F::Init)?, depth + 1)
            }
            // Either side may be what it holds.
            K::Conditional => [F::Consequent, F::Alternate].iter().find_map(|&f| self.global_value_at(s.get(node, f)?, depth + 1)),
            K::Logical => [F::Left, F::Right].iter().find_map(|&f| self.global_value_at(s.get(node, f)?, depth + 1)),
            K::Member | K::Index => {
                let object = self.global_value_at(s.get(node, F::Object)?, depth + 1)?;
                if !GLOBAL_OBJECTS.contains(&object.as_str()) {
                    return None;
                }
                super::statics::property_name(s, node)
            }
            _ => None,
        }
    }
}
