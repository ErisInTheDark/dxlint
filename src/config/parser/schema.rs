//! Option schemas, in TypeScript-like type syntax: `option name <schema> =
//! <default> "doc"` lines and `schema name <schema> "doc"` items.
//!
//! ```text
//! schema := term ('|' term)*
//! term   := atom ('[' range? ']')*            T[], T[1..]
//! atom   := (int | number | string) ('(' range ')')?
//!         | boolean | any | null | regex | js_regex | literal | name
//!         | 'enum(' word (',' word)* ')'           bare-word members
//!         | '(' schema ')' | '{' member* '}'
//! range  := num? '..' num?
//! member := key '?'? ':' schema ('=' literal)? doc?
//!         | '[' ident ':' string ']' ':' schema   any other key
//! ```

use super::super::ast::{Expr, ExprKind, Name, OptionDef, UnOp, Use};
use super::super::error::ConfigError;
use super::super::lexer::{decode_escapes, Tk};
use super::super::schema::{FieldDefault, Schema, SchemaDef, SchemaField};
use super::Parser;
use crate::expr::Value;

const NEW_FORM: &str = "write options as `option name <schema> = <default> \"description\"`, e.g. `option max int(0..) = 2 \"blank lines in a row\"`";

/// A constant value written as a literal: strings without holes, numbers,
/// booleans, null, and lists and records of those.
pub(super) fn literal(e: &Expr) -> Result<Value<'static>, ConfigError> {
    Ok(match &e.kind {
        ExprKind::Str(s) => Value::from(s.clone()),
        ExprKind::Num(n) => Value::Num(*n),
        ExprKind::Bool(b) => Value::Bool(*b),
        ExprKind::Null => Value::Null,
        ExprKind::Unary(UnOp::Neg, inner) if matches!(inner.kind, ExprKind::Num(_)) => Value::Num(-literal(inner)?.num().unwrap_or(0.0)),
        ExprKind::List(items) => Value::List(items.iter().map(literal).collect::<Result<_, _>>()?),
        ExprKind::Record(entries) => {
            Value::Record(entries.iter().map(|(k, v)| Ok((k.text.clone().into(), literal(v)?))).collect::<Result<_, ConfigError>>()?)
        }
        _ => {
            return Err(ConfigError::new(e.span, "defaults are literal values")
                .note("strings, numbers, true, false, null, and lists or records of them"))
        }
    })
}

type Range = (Option<f64>, Option<f64>);

impl Parser<'_> {
    /// `schema name <schema> "doc"`
    pub(super) fn schema_item(&mut self) {
        self.bump();
        let overriding = self.overriding.take();
        let Some(name) = self.ident("a schema name") else { return };
        let Some(schema) = self.schema() else { return };
        let doc = self.doc();
        self.end_of_item();
        if let Some(prev) = self.config.schemas.iter().position(|s| s.name.text == name.text) {
            if overriding.is_some() {
                self.config.schemas.remove(prev);
            } else {
                self.errors.push(
                    ConfigError::new(name.span, format!("schema `{}` is already defined", name.text))
                        .related(self.config.schemas[prev].name.span, "first defined here")
                        .note("write `override schema ...` to replace it"),
                );
                return;
            }
        }
        let library = self.defining > 0;
        self.config.schemas.push(SchemaDef { name, schema, doc, library });
    }

    /// After `option`: `name <schema> = <default> "doc"`.
    pub(super) fn option_line(&mut self) -> Option<OptionDef> {
        let name = self.ident("an option name")?;
        let schema = self.schema()?;
        self.skip(false, true);
        if !self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::Eq) {
            let span = self.here();
            self.errors.push(ConfigError::new(span, format!("option `{}` needs a default: `= <value>`", name.text)).note(NEW_FORM));
            return None;
        }
        self.ts.next_raw();
        let default = self.operand()?;
        let value = match self.enum_word(&default, &schema) {
            Some(word) => Value::from(word),
            None => literal(&default).map_err(|e| self.errors.push(e)).ok()?,
        };
        let description = self.doc();
        Some(OptionDef { name, schema, default: value, default_span: default.span, description, primary: false })
    }

    /// A bare word the schema takes as an enum member (`= never`), as text.
    pub(super) fn enum_word(&self, e: &Expr, schema: &Schema) -> Option<String> {
        let ExprKind::Var(path) = &e.kind else { return None };
        let [word] = path.as_slice() else { return None };
        let named = |name: &str| self.config.schemas.iter().find(|s| s.name.text == name).map(|s| &s.schema);
        schema.takes_word(&word.text, &named).then(|| word.text.clone())
    }

    /// An optional `"doc"` string next on the line.
    fn doc(&mut self) -> Option<String> {
        self.skip(false, true);
        if self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::Str) {
            self.string("a description").map(|(s, _)| s)
        } else {
            None
        }
    }

    fn next_is(&mut self, kind: Tk) -> bool {
        self.skip(false, true);
        self.ts.peek_raw(0).is_some_and(|t| t.kind == kind)
    }

    fn close(&mut self, kind: Tk, what: &str) -> Option<()> {
        self.skip(true, true);
        if self.ts.peek_raw(0).is_some_and(|t| t.kind == kind) {
            self.ts.next_raw();
            Some(())
        } else {
            self.error_here(format!("expected {what}"));
            None
        }
    }

    /// Is the next meaningful token a `|`? Unions may continue on the next
    /// line when it starts with one.
    fn union_continues(&mut self) -> bool {
        let mut i = 0;
        while let Some(t) = self.ts.peek_raw(i) {
            match t.kind {
                Tk::Newline | Tk::InvOpen | Tk::InvClose => i += 1,
                Tk::Pipe => {
                    for _ in 0..=i {
                        self.ts.next_raw();
                    }
                    return true;
                }
                _ => return false,
            }
        }
        false
    }

    pub(super) fn schema(&mut self) -> Option<Schema> {
        let mut options = vec![self.schema_term()?];
        while self.union_continues() {
            options.push(self.schema_term()?);
        }
        Some(if options.len() == 1 { options.pop().expect("one") } else { Schema::Union(options) })
    }

    /// An atom, then `[]` / `[1..]` making lists of it.
    fn schema_term(&mut self) -> Option<Schema> {
        let mut s = self.schema_atom()?;
        while self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::LBrack) {
            self.ts.next_raw();
            let range = if self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::RBrack) { None } else { Some(self.range()?) };
            self.close(Tk::RBrack, "`]` closing a list type")?;
            s = Schema::List(Box::new(s));
            if let Some((min, max)) = range {
                s = Schema::Range(Box::new(s), min, max);
            }
        }
        Some(s)
    }

    /// `0..`, `..10`, `1..3`.
    fn range(&mut self) -> Option<Range> {
        self.skip(true, true);
        let number = |p: &mut Self| -> Option<f64> {
            let neg = p.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::Minus) && p.ts.peek_raw(1).is_some_and(|t| t.kind == Tk::Num);
            if neg {
                p.ts.next_raw();
            }
            let t = p.ts.peek_raw(0).filter(|t| t.kind == Tk::Num)?.clone();
            p.ts.next_raw();
            let n: f64 = t.text.parse().unwrap_or(0.0);
            Some(if neg { -n } else { n })
        };
        let min = number(self);
        let dots = self.ts.peek_raw(0).is_some_and(|t| t.kind == Tk::Dot) && self.ts.peek_raw(1).is_some_and(|t| t.kind == Tk::Dot);
        if !dots {
            self.error_here("expected a range: `0..`, `..10` or `1..10`");
            return None;
        }
        self.ts.next_raw();
        self.ts.next_raw();
        let max = number(self);
        Some((min, max))
    }

    fn schema_atom(&mut self) -> Option<Schema> {
        self.skip(false, true);
        let Some(t) = self.ts.peek_raw(0).cloned() else {
            self.error_here("expected a type, found end of file");
            return None;
        };
        let lit = |v: Value<'static>| Some(Schema::Lit(v));
        match t.kind {
            Tk::Str => {
                self.ts.next_raw();
                match decode_escapes(&t.text, t.span) {
                    Ok(s) => lit(Value::from(s)),
                    Err(e) => {
                        self.errors.push(e);
                        None
                    }
                }
            }
            Tk::Num => {
                self.ts.next_raw();
                lit(Value::Num(t.text.parse().unwrap_or(0.0)))
            }
            Tk::Minus if self.ts.peek_raw(1).is_some_and(|n| n.kind == Tk::Num) => {
                self.ts.next_raw();
                let n = self.ts.next_raw().expect("peeked");
                lit(Value::Num(-n.text.parse::<f64>().unwrap_or(0.0)))
            }
            Tk::LParen => {
                self.ts.next_raw();
                self.skip(true, true);
                let inner = self.schema()?;
                self.close(Tk::RParen, "`)`")?;
                Some(inner)
            }
            Tk::LBrace => {
                self.ts.next_raw();
                self.record_schema()
            }
            Tk::Ident if &*t.text == "enum" && self.ts.peek_raw(1).is_some_and(|p| p.kind == Tk::LParen && t.touches(p)) => {
                self.ts.next_raw();
                self.ts.next_raw();
                self.enum_words()
            }
            Tk::Ident => {
                self.ts.next_raw();
                let s = match &*t.text {
                    "string" => Schema::Str,
                    "number" => Schema::Num,
                    "int" => Schema::Int,
                    "boolean" => Schema::Bool,
                    "any" => Schema::Any,
                    "null" => Schema::Null,
                    "regex" => Schema::Regex,
                    "js_regex" => Schema::JsRegex,
                    "true" => Schema::Lit(Value::Bool(true)),
                    "false" => Schema::Lit(Value::Bool(false)),
                    "str" | "num" | "bool" => {
                        let ts = match &*t.text {
                            "str" => "string",
                            "num" => "number",
                            _ => "boolean",
                        };
                        self.errors.push(ConfigError::new(t.span, format!("unknown type `{}`; did you mean `{ts}`?", t.text)));
                        return None;
                    }
                    _ => {
                        let name = Name { text: t.text.to_string(), span: t.span };
                        self.config.uses.push(Use::Schema(name.clone()));
                        return Some(Schema::Named(name));
                    }
                };
                // `int(0..)`, `number(0..1)`, `string(1..)`: a range.
                if matches!(s, Schema::Int | Schema::Num | Schema::Str) && self.ts.peek_raw(0).is_some_and(|p| p.kind == Tk::LParen && t.touches(p)) {
                    self.ts.next_raw();
                    let (min, max) = self.range()?;
                    self.close(Tk::RParen, "`)` closing the range")?;
                    return Some(Schema::Range(Box::new(s), min, max));
                }
                Some(s)
            }
            _ => {
                self.errors.push(ConfigError::new(t.span, format!("expected a type, found {}", t.describe())).note(
                    "types: string, number, int, boolean, any, null, regex, js_regex, enum(a, b), a literal (\"x\", 3, true), T[], { key: T }, a | b, int(0..), or a `schema` name",
                ));
                None
            }
        }
    }

    /// `always, never, always-multiline)` after `enum(`: bare words.
    fn enum_words(&mut self) -> Option<Schema> {
        let mut words: Vec<String> = Vec::new();
        loop {
            self.skip(true, true);
            let Some(t) = self.ts.next_raw() else {
                self.error_here("unclosed `enum(`");
                return None;
            };
            match t.kind {
                Tk::RParen if !words.is_empty() => break,
                Tk::Comma if !words.is_empty() => continue,
                Tk::Ident if !words.contains(&t.text.to_string()) => words.push(t.text.to_string()),
                Tk::Ident => {
                    self.errors.push(ConfigError::new(t.span, format!("`{}` appears twice in this enum", t.text)));
                    return None;
                }
                _ => {
                    self.errors.push(
                        ConfigError::new(t.span, format!("expected a word in `enum(...)`, found {}", t.describe()))
                            .note("enum members are bare words: `enum(always, never, always-multiline)`"),
                    );
                    return None;
                }
            }
        }
        Some(Schema::Enum(words))
    }

    /// `{ key: T = default "doc"; ... }` after its `{`.
    fn record_schema(&mut self) -> Option<Schema> {
        let mut fields: Vec<SchemaField> = Vec::new();
        let mut rest = None;
        loop {
            self.skip(true, true);
            let Some(t) = self.ts.next_raw() else {
                self.error_here("unclosed `{` in a type");
                return None;
            };
            let key = match t.kind {
                Tk::RBrace => break,
                Tk::Comma => continue,
                Tk::Ident => t.text.to_string(),
                Tk::Str => match decode_escapes(&t.text, t.span) {
                    Ok(s) => s,
                    Err(e) => {
                        self.errors.push(e);
                        return None;
                    }
                },
                // `[key: string]: T`
                Tk::LBrack => {
                    self.ident("a key name")?;
                    if !self.next_is(Tk::Colon) {
                        self.error_here("expected `: string]` in an index signature");
                        return None;
                    }
                    self.ts.next_raw();
                    self.skip(false, true);
                    if !self.ts.peek_raw(0).is_some_and(|s| s.is_ident("string")) {
                        self.error_here("index signatures take `string` keys: `[key: string]: T`");
                        return None;
                    }
                    self.ts.next_raw();
                    self.close(Tk::RBrack, "`]`")?;
                    if !self.next_is(Tk::Colon) {
                        self.error_here("expected `:` and the values' type");
                        return None;
                    }
                    self.ts.next_raw();
                    rest = Some(Box::new(self.schema()?));
                    continue;
                }
                _ => {
                    self.errors.push(ConfigError::new(t.span, format!("expected a key or `}}` in a record type, found {}", t.describe())));
                    return None;
                }
            };
            let optional = self.ts.peek_raw(0).is_some_and(|q| q.kind == Tk::Question);
            if optional {
                self.ts.next_raw();
            }
            if !self.next_is(Tk::Colon) {
                self.error_here(format!("expected `:` and a type after `{key}`"));
                return None;
            }
            self.ts.next_raw();
            let schema = self.schema()?;
            if fields.iter().any(|f| f.name == key) {
                self.errors.push(ConfigError::new(t.span, format!("`{key}` appears twice in this record type")));
                return None;
            }
            let default = if self.next_is(Tk::Eq) {
                self.ts.next_raw();
                let e = self.operand()?;
                match literal(&e) {
                    Ok(v) => FieldDefault::Value(v),
                    Err(err) => {
                        self.errors.push(err);
                        return None;
                    }
                }
            } else if optional {
                FieldDefault::Null
            } else {
                FieldDefault::Required
            };
            let doc = self.doc();
            fields.push(SchemaField { name: key, schema, default, doc });
        }
        Some(Schema::Record(fields, rest))
    }
}
