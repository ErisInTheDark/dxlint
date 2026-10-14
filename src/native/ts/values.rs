//! Which global an expression stands for, following what eslint's
//! `ReferenceTracker` follows: the global itself (`Math`), through the
//! global object (`globalThis.Math`), and through variables that only ever
//! hold it (`const m = Math`).

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};

/// Names of the global object.
const GLOBAL_OBJECTS: &[&str] = &["globalThis", "global", "self", "window"];

impl NativeFile<'_> {
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
                            out.push_str(&crate::expr::js::strings::decode_literal(&format!("`{piece}`"))?);
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
            // `(a, "x")`, `y = "x"`: the last value, the assigned value.
            K::Sequence => self.static_string_at(s.all(node, F::Expr).last()?, depth + 1),
            K::Assign if s.word(node, F::Op) == Some("=") => self.static_string_at(s.get(node, F::Right)?, depth + 1),
            // `"gimu"[0]`: one UTF-16 unit, as JavaScript indexes.
            K::Index if s.get(node, F::Optional).is_none() => {
                let text = self.static_string_at(s.get(node, F::Object)?, depth + 1)?;
                let at = self.static_number_at(s.get(node, F::Index)?, depth + 1)?;
                let unit = (at >= 0.0 && at.fract() == 0.0).then(|| text.encode_utf16().nth(at as usize)).flatten()?;
                Some(String::from_utf16_lossy(&[unit]))
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

#[cfg(test)]
mod tests {
    #[test]
    fn static_strings_follow_sequences_assignments_and_indexes() {
        let text = "const flags = 'gimu'; let y; /*@*/(0, 'a'); /*@*/y = 'b'; /*@*/flags[1]; /*@*/'😀'[0]; /*@*/flags[9];";
        crate::engine::tests::with_native_file(text, |file, marks| {
            // The marks are expression statements.
            let expr = |m| file.syntax.get(m, crate::native::ts::kinds::Field::Expr).expect("an expression");
            let got: Vec<_> = marks.iter().map(|&m| file.static_string(expr(m))).collect();
            assert_eq!(got, [Some("a".into()), Some("b".into()), Some("i".into()), Some("\u{fffd}".into()), None]);
        })
    }
}
