//! Expressions whose value (or truthiness, or nullishness) cannot change,
//! as eslint's `isConstant` and no-constant-binary-expression decide.
//! Parentheses are not nodes to them; TypeScript wrappers (`as`, `!`) are
//! and end the reasoning.

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};

/// What no-constant-binary-expression reports about a `==`-like comparison
/// or a `&&`/`||`/`??`: the message id, the node it points at, and the
/// operator / other side / property the message names.
pub struct ConstantComparison {
    pub kind: &'static str,
    pub node: u32,
    pub detail: &'static str,
}

impl NativeFile<'_> {
    fn bare(&self, node: u32) -> u32 {
        self.syntax.unparen(node)
    }

    /// A reference to the global (builtin or configured) of that name.
    fn global_named(&self, node: u32, name: &str) -> bool {
        self.syntax.is(node, K::Ident) && self.syntax.text(node) == name && self.global_setting(node).is_some_and(|g| g != "off")
    }

    fn is_literal(&self, node: u32) -> bool {
        matches!(self.syntax.kind(node), Some(K::String | K::Number | K::Bool | K::Null | K::Regex | K::BigInt))
    }

    fn is_static_template(&self, node: u32) -> bool {
        self.syntax.is(node, K::Template) && self.syntax.all(node, F::Expr).next().is_none()
    }

    /// The callee of a call, when it is a plain identifier.
    fn callee_name(&self, call: u32) -> Option<(u32, &str)> {
        let c = self.bare(self.syntax.get(call, F::Callee)?);
        self.syntax.is(c, K::Ident).then(|| (c, self.syntax.text(c)))
    }

    /// eslint's `isConstant`: the node's truthiness (`in_boolean`) or its
    /// value as a string or number never changes.
    pub fn is_constant(&self, node: u32, in_boolean: bool) -> bool {
        let s = &self.syntax;
        let node = self.bare(node);
        let Some(kind) = s.kind(node) else { return false };
        match kind {
            K::String | K::Number | K::Bool | K::Null | K::Regex | K::BigInt | K::Arrow | K::Function | K::Class | K::Object => true,
            K::Hole => true,
            K::Template => {
                let quasi_text = s.all(node, F::Quasi).any(|q| {
                    let t = s.text(q).trim_start_matches(['`', '}']).trim_end_matches('`');
                    let t = t.strip_suffix("${").unwrap_or(t);
                    super::regex::cook(&format!("`{t}`")).is_none_or(|cooked| !cooked.is_empty())
                });
                in_boolean && quasi_text || s.all(node, F::Expr).all(|e| self.is_constant(e, false))
            }
            K::Array => in_boolean || s.all(node, F::Element).all(|e| self.is_constant(e, false)),
            K::Unary => {
                let op = s.word(node, F::Op).unwrap_or("");
                if op == "void" || op == "typeof" && in_boolean {
                    return true;
                }
                let Some(arg) = s.get(node, F::Arg) else { return false };
                self.is_constant(arg, op == "!")
            }
            K::Binary => {
                s.word(node, F::Op) != Some("in")
                    && s.get(node, F::Left).is_some_and(|l| self.is_constant(l, false))
                    && s.get(node, F::Right).is_some_and(|r| self.is_constant(r, false))
            }
            K::Logical => {
                let op = s.word(node, F::Op).unwrap_or("");
                let (Some(l), Some(r)) = (s.get(node, F::Left), s.get(node, F::Right)) else { return false };
                let (lc, rc) = (self.is_constant(l, in_boolean), self.is_constant(r, in_boolean));
                lc && (rc || self.is_logical_identity(l, op)) || in_boolean && rc && self.is_logical_identity(r, op)
            }
            K::New => in_boolean,
            K::Assign => match s.word(node, F::Op) {
                Some("=") => s.get(node, F::Right).is_some_and(|r| self.is_constant(r, in_boolean)),
                Some(op @ ("||=" | "&&=")) if in_boolean => s.get(node, F::Right).is_some_and(|r| self.is_logical_identity(r, &op[..2])),
                _ => false,
            },
            K::Sequence => s.all(node, F::Expr).last().is_some_and(|e| self.is_constant(e, in_boolean)),
            K::Spread => s.get(node, F::Arg).is_some_and(|a| self.is_constant(a, in_boolean)),
            K::Call => match self.callee_name(node) {
                Some((callee, "Boolean")) => {
                    let first = s.all(node, F::Arg).next();
                    first.is_none_or(|a| self.is_constant(a, true)) && self.global_named(callee, "Boolean")
                }
                _ => false,
            },
            K::Ident => self.global_named(node, "undefined"),
            _ => false,
        }
    }

    /// eslint's `isLogicalIdentity`: the node decides `op` by itself.
    fn is_logical_identity(&self, node: u32, op: &str) -> bool {
        let s = &self.syntax;
        let node = self.bare(node);
        match s.kind(node) {
            Some(K::String | K::Number | K::Bool | K::Null | K::Regex | K::BigInt) => match truthy_literal(s.kind(node), s.text(node)) {
                Some(t) => op == "||" && t || op == "&&" && !t,
                None => false,
            },
            Some(K::Unary) => op == "&&" && s.word(node, F::Op) == Some("void"),
            Some(K::Logical) => {
                s.word(node, F::Op) == Some(op) && [F::Left, F::Right].iter().any(|&f| s.get(node, f).is_some_and(|x| self.is_logical_identity(x, op)))
            }
            Some(K::Assign) => match s.word(node, F::Op) {
                Some(a @ ("||=" | "&&=")) => &a[..2] == op && s.get(node, F::Right).is_some_and(|r| self.is_logical_identity(r, op)),
                _ => false,
            },
            _ => false,
        }
    }

    fn is_null_or_undefined(&self, node: u32) -> bool {
        let s = &self.syntax;
        let node = self.bare(node);
        s.is(node, K::Null) || self.global_named(node, "undefined") || s.is(node, K::Unary) && s.word(node, F::Op) == Some("void")
    }

    fn has_constant_nullishness(&self, node: u32, non_nullish: bool) -> bool {
        let s = &self.syntax;
        if non_nullish && self.is_null_or_undefined(node) {
            return false;
        }
        let node = self.bare(node);
        match s.kind(node) {
            Some(K::Object | K::Array | K::Arrow | K::Function | K::Class | K::New | K::Template | K::Update | K::Binary | K::Unary) => true,
            Some(k) if self.is_literal(node) || k == K::Regex => true,
            Some(K::Call) => match self.callee_name(node) {
                Some((c, name @ ("Boolean" | "String" | "Number" | "Symbol" | "BigInt"))) => self.global_named(c, name),
                _ => false,
            },
            Some(K::Logical) => s.word(node, F::Op) == Some("??") && s.get(node, F::Right).is_some_and(|r| self.has_constant_nullishness(r, true)),
            Some(K::Assign) => match s.word(node, F::Op) {
                Some("=") => s.get(node, F::Right).is_some_and(|r| self.has_constant_nullishness(r, non_nullish)),
                Some("||=" | "&&=" | "??=") => false,
                _ => true,
            },
            Some(K::Sequence) => s.all(node, F::Expr).last().is_some_and(|e| self.has_constant_nullishness(e, non_nullish)),
            Some(K::Ident) => self.global_named(node, "undefined"),
            _ => false,
        }
    }

    fn is_static_boolean(&self, node: u32) -> bool {
        let s = &self.syntax;
        let node = self.bare(node);
        match s.kind(node) {
            Some(K::Bool) => true,
            Some(K::Call) => match self.callee_name(node) {
                Some((c, "Boolean")) => self.global_named(c, "Boolean") && s.all(node, F::Arg).next().is_none_or(|a| self.is_constant(a, true)),
                _ => false,
            },
            Some(K::Unary) => s.word(node, F::Op) == Some("!") && s.get(node, F::Arg).is_some_and(|a| self.is_constant(a, true)),
            _ => false,
        }
    }

    fn has_constant_loose_boolean_comparison(&self, node: u32) -> bool {
        let s = &self.syntax;
        let node = self.bare(node);
        match s.kind(node) {
            Some(K::Object | K::Class | K::Arrow | K::Function) => true,
            Some(K::Array) => {
                let elements: Vec<u32> = s.all(node, F::Element).collect();
                let solid = elements.iter().filter(|&&e| !matches!(s.kind(e), Some(K::Hole | K::Spread))).count();
                elements.is_empty() || solid > 1
            }
            Some(K::Unary) => match s.word(node, F::Op) {
                Some("void" | "typeof") => true,
                Some("!") => s.get(node, F::Arg).is_some_and(|a| self.is_constant(a, true)),
                _ => false,
            },
            Some(K::Call) => match self.callee_name(node) {
                Some((c, "Boolean")) if self.global_named(c, "Boolean") => s.all(node, F::Arg).next().is_none_or(|a| self.is_constant(a, true)),
                _ => false,
            },
            Some(K::Ident) => self.global_named(node, "undefined"),
            Some(K::Template) => self.is_static_template(node),
            Some(K::Assign) => s.word(node, F::Op) == Some("=") && s.get(node, F::Right).is_some_and(|r| self.has_constant_loose_boolean_comparison(r)),
            Some(K::Sequence) => s.all(node, F::Expr).last().is_some_and(|e| self.has_constant_loose_boolean_comparison(e)),
            _ => self.is_literal(node),
        }
    }

    fn has_constant_strict_boolean_comparison(&self, node: u32) -> bool {
        let s = &self.syntax;
        let node = self.bare(node);
        match s.kind(node) {
            Some(K::Object | K::Array | K::Arrow | K::Function | K::Class | K::New | K::Template | K::Update) => true,
            Some(K::Binary) => matches!(s.word(node, F::Op), Some("+" | "-" | "*" | "/" | "%" | "|" | "^" | "&" | "**" | "<<" | ">>" | ">>>")),
            Some(K::Unary) => match s.word(node, F::Op) {
                Some("delete") => false,
                Some("!") => s.get(node, F::Arg).is_some_and(|a| self.is_constant(a, true)),
                _ => true,
            },
            Some(K::Sequence) => s.all(node, F::Expr).last().is_some_and(|e| self.has_constant_strict_boolean_comparison(e)),
            Some(K::Ident) => self.global_named(node, "undefined"),
            Some(K::Assign) => match s.word(node, F::Op) {
                Some("=") => s.get(node, F::Right).is_some_and(|r| self.has_constant_strict_boolean_comparison(r)),
                Some("||=" | "&&=" | "??=") => false,
                _ => true,
            },
            Some(K::Call) => match self.callee_name(node) {
                Some((c, name @ ("String" | "Number" | "BigInt" | "Symbol"))) => self.global_named(c, name),
                Some((c, "Boolean")) if self.global_named(c, "Boolean") => s.all(node, F::Arg).next().is_none_or(|a| self.is_constant(a, true)),
                _ => false,
            },
            _ => self.is_literal(node),
        }
    }

    fn is_always_new(&self, node: u32) -> bool {
        let s = &self.syntax;
        let node = self.bare(node);
        match s.kind(node) {
            Some(K::Object | K::Array | K::Arrow | K::Function | K::Class | K::Regex) => true,
            Some(K::New) => match s.get(node, F::Callee).map(|c| self.bare(c)) {
                Some(c) if s.is(c, K::Ident) => self.globals().is_builtin(s.text(c)) && self.global_named(c, s.text(c)),
                _ => false,
            },
            Some(K::Sequence) => s.all(node, F::Expr).last().is_some_and(|e| self.is_always_new(e)),
            Some(K::Assign) => s.word(node, F::Op) == Some("=") && s.get(node, F::Right).is_some_and(|r| self.is_always_new(r)),
            Some(K::Conditional) => [F::Consequent, F::Alternate].iter().all(|&f| s.get(node, f).is_some_and(|x| self.is_always_new(x))),
            _ => false,
        }
    }

    fn binary_constant_operand(&self, a: u32, b: u32, op: &str) -> Option<u32> {
        let loose = matches!(op, "==" | "!=");
        let strict = matches!(op, "===" | "!==");
        if !loose && !strict {
            return None;
        }
        let boolean = if loose { self.has_constant_loose_boolean_comparison(b) } else { self.has_constant_strict_boolean_comparison(b) };
        (self.is_null_or_undefined(a) && self.has_constant_nullishness(b, false) || self.is_static_boolean(a) && boolean).then_some(b)
    }

    fn is_static_literal(&self, node: u32) -> bool {
        let s = &self.syntax;
        let node = self.bare(node);
        match s.kind(node) {
            Some(K::Unary) => matches!(s.word(node, F::Op), Some("-" | "+" | "~")) && s.get(node, F::Arg).is_some_and(|a| self.is_literal(a)),
            Some(K::Ident) => self.global_named(node, "undefined"),
            Some(K::Template) => self.is_static_template(node),
            _ => self.is_literal(node),
        }
    }

    /// no-constant-binary-expression on a binary or logical expression
    /// (`relational`: also `<` and friends between literals).
    pub fn constant_comparison(&self, node: u32, relational: bool) -> Option<ConstantComparison> {
        let s = &self.syntax;
        let op = s.word(node, F::Op)?;
        let (left, right) = (s.get(node, F::Left)?, s.get(node, F::Right)?);
        if s.is(node, K::Logical) {
            if matches!(op, "&&" | "||") && self.is_constant(left, true) {
                return Some(ConstantComparison { kind: "constantShortCircuit", node: self.bare(left), detail: "truthiness" });
            }
            if op == "??" && self.has_constant_nullishness(left, false) {
                return Some(ConstantComparison { kind: "constantShortCircuit", node: self.bare(left), detail: "nullishness" });
            }
            return None;
        }
        if !s.is(node, K::Binary) {
            return None;
        }
        if let Some(n) = self.binary_constant_operand(left, right, op) {
            return Some(ConstantComparison { kind: "constantBinaryOperand", node: self.bare(n), detail: "left" });
        }
        if let Some(n) = self.binary_constant_operand(right, left, op) {
            return Some(ConstantComparison { kind: "constantBinaryOperand", node: self.bare(n), detail: "right" });
        }
        match op {
            "===" | "!==" => {
                if self.is_always_new(left) {
                    return Some(ConstantComparison { kind: "alwaysNew", node: self.bare(left), detail: "" });
                }
                if self.is_always_new(right) {
                    return Some(ConstantComparison { kind: "alwaysNew", node: self.bare(right), detail: "" });
                }
            }
            "==" | "!=" if self.is_always_new(left) && self.is_always_new(right) => {
                return Some(ConstantComparison { kind: "bothAlwaysNew", node: self.bare(left), detail: "" });
            }
            "<" | "<=" | ">" | ">=" if relational && self.is_static_literal(left) && self.is_static_literal(right) => {
                return Some(ConstantComparison { kind: "constantRelationalComparison", node, detail: "" });
            }
            _ => {}
        }
        None
    }
}

/// A literal's truthiness (`None` for what is not a literal).
fn truthy_literal(kind: Option<K>, text: &str) -> Option<bool> {
    Some(match kind? {
        K::Bool => text == "true",
        K::Null => false,
        K::Number => super::statics::number(text).is_some_and(|n| n != 0.0 && !n.is_nan()),
        K::BigInt => super::statics::number(text.trim_end_matches('n')).is_some_and(|n| n != 0.0),
        K::String => text.len() > 2,
        K::Regex => true,
        _ => return None,
    })
}
