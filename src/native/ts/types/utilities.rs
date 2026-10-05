//! Built-in utility types whose mapped or intersection syntax needs a
//! semantic owner instead of the general syntax evaluator.

use std::sync::Arc;

use super::{Checker, Env, Sym, Ty};
use super::super::summary::DeclKind;

#[derive(Clone, Copy)]
enum StringTransform { Upper, Lower, Capitalize, Uncapitalize }

impl StringTransform {
    fn text(self, text: &str) -> String {
        match self {
            Self::Upper => text.to_uppercase(),
            Self::Lower => text.to_lowercase(),
            Self::Capitalize | Self::Uncapitalize => {
                let Some(first) = text.chars().next() else { return String::new() };
                // TypeScript maps the first UTF-16 code unit, not an astral character.
                if first.len_utf16() != 1 { return text.to_owned() }
                let mut mapped = match self {
                    Self::Capitalize => first.to_uppercase().collect::<String>(),
                    _ => first.to_lowercase().collect(),
                };
                mapped.push_str(&text[first.len_utf8()..]);
                mapped
            }
        }
    }

    fn ty(self, ty: &Ty) -> Ty {
        match ty {
            Ty::Any | Ty::Never | Ty::Prim("string") => ty.clone(),
            Ty::Lit(text, "string") => super::super::regex::cook(text)
                .map_or(Ty::Unknown, |text| Ty::Lit(format!("{:?}", self.text(&text)).into(), "string")),
            Ty::Union(parts) => Ty::union(parts.iter().map(|part| self.ty(part)).collect()),
            _ => Ty::Unknown,
        }
    }
}

pub(super) struct PropertyFilter<'a> {
    pub source: &'a Ty,
    keys: &'a Ty,
    omit: bool,
}

impl PropertyFilter<'_> {
    pub fn allows(&self, name: &str) -> Option<bool> {
        let matches = if name.starts_with("[Symbol.") { accepts_symbol(self.keys) }
            else if name.starts_with('[') { None }
            else { accepts_key(self.keys, name) };
        matches.map(|matches| matches != self.omit)
    }

    pub fn names(&self, ck: &Checker<'_>, depth: usize) -> Option<Vec<Box<str>>> {
        if !self.omit { return finite_keys(self.keys).map(|names| names.into_iter().map(String::into_boxed_str).collect()) }
        required_keys(self.keys)?;
        ck.known_property_names(self.source, depth)?.into_iter().try_fold(Vec::new(), |mut names, name| {
            if self.allows(&name)? { names.push(name); }
            Some(names)
        })
    }
}

impl Checker<'_> {
    /// Resolved empty property bags only: unresolved mapped keys are not proof.
    pub fn generated_empty_object(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Obj(members, _) => members.is_empty(),
            Ty::Struct(properties) => properties.is_empty(),
            Ty::Union(parts) => parts.iter().any(|part| self.generated_empty_object(part)),
            Ty::Inter(parts) => !parts.is_empty() && parts.iter().all(|part| self.generated_empty_object(part)),
            _ => self.property_filter(ty).and_then(|filter| filter.names(self, 0)).is_some_and(|names| names.is_empty())
                || self.record_parts(ty).and_then(|(keys, _)| finite_keys(keys)).is_some_and(|names| names.is_empty()),
        }
    }

    pub(super) fn utility_type(&self, sym: &[super::super::modules::DeclRef], args: &[Ty], env: &Env) -> Option<Ty> {
        if args.len() == 1 {
            for (name, transform) in [("Uppercase", StringTransform::Upper), ("Lowercase", StringTransform::Lower), ("Capitalize", StringTransform::Capitalize), ("Uncapitalize", StringTransform::Uncapitalize)] {
                if self.is_builtin_symbol(sym, name) { return Some(transform.ty(&args[0])) }
            }
        }
        if self.is_builtin_symbol(sym, "Awaited") && args.len() == 1 {
            return Some(self.awaited(&args[0]));
        }
        if self.is_builtin_symbol(sym, "NonNullable") && args.len() == 1 {
            let empty = || Ty::Obj(Arc::new(Vec::new()), Arc::new(env.clone()));
            return Some(match &args[0] {
                Ty::IntrinsicUnknown => empty(),
                Ty::Param(_) => Ty::Inter(vec![args[0].clone(), empty()]),
                ty => ty.clone().non_null(),
            });
        }
        if args.len() == 1 && self.is_builtin_symbol(sym, "Readonly") {
            return Some(Ty::Named(Sym::from(sym), args.to_vec()));
        }
        if args.len() == 2 && ["Record", "Pick", "Omit"].iter().any(|name| self.is_builtin_symbol(sym, name)) {
            return Some(Ty::Named(Sym::from(sym), args.to_vec()));
        }
        None
    }

    pub fn readonly_source<'a>(&self, ty: &'a Ty) -> Option<&'a Ty> {
        match ty {
            Ty::Named(symbols, args) if args.len() == 1 && self.is_builtin_symbol(symbols, "Readonly") => Some(&args[0]),
            _ => None,
        }
    }

    pub(super) fn property_filter<'a>(&self, ty: &'a Ty) -> Option<PropertyFilter<'a>> {
        let Ty::Named(symbols, args) = ty else { return None };
        if args.len() != 2 { return None }
        let omit = if self.is_builtin_symbol(symbols, "Omit") { true }
            else if self.is_builtin_symbol(symbols, "Pick") { false }
            else { return None };
        Some(PropertyFilter { source: &args[0], keys: &args[1], omit })
    }

    pub(super) fn known_property_names(&self, ty: &Ty, depth: usize) -> Option<Vec<Box<str>>> {
        if depth >= 32 { return None }
        if let Some(source) = self.readonly_source(ty) { return self.known_property_names(source, depth + 1) }
        if let Some(filter) = self.property_filter(ty) { return filter.names(self, depth + 1) }
        if let Some((keys, _)) = self.record_parts(ty) { return finite_keys(keys).map(|names| names.into_iter().map(String::into_boxed_str).collect()) }
        match ty {
            Ty::Null | Ty::Undefined | Ty::Void => Some(Vec::new()),
            Ty::Union(parts) => {
                let mut parts = parts.iter();
                let mut names = self.known_property_names(parts.next()?, depth + 1)?;
                for part in parts {
                    let other = self.known_property_names(part, depth + 1)?;
                    names.retain(|name| other.contains(name));
                }
                Some(names)
            }
            Ty::Inter(parts) => parts.iter().try_fold(Vec::new(), |mut names, part| {
                for name in self.known_property_names(part, depth + 1)? { if !names.contains(&name) { names.push(name); } }
                Some(names)
            }),
            Ty::Obj(..) | Ty::Struct(_) => {
                let mut names = Vec::new();
                self.member_names(ty, &mut names);
                (!names.iter().any(|name| matches!(&**name, "[index]" | "()"))).then_some(names)
            }
            Ty::Named(symbols, _) if !self.is_enum(symbols) && symbols.iter().all(|reference| self.summary_of(reference.file).is_some_and(|summary| matches!(summary.decls[reference.decl as usize].kind, DeclKind::Class | DeclKind::Interface))) => {
                let mut names = Vec::new();
                self.member_names(ty, &mut names);
                (!names.iter().any(|name| matches!(&**name, "[index]" | "()"))).then_some(names)
            }
            _ => None,
        }
    }

    pub(super) fn record_parts<'a>(&self, ty: &'a Ty) -> Option<(&'a Ty, &'a Ty)> {
        match ty {
            Ty::Named(sym, args) if args.len() == 2 && self.is_builtin_symbol(sym, "Record") => Some((&args[0], &args[1])),
            _ => None,
        }
    }
}

fn finite_keys(ty: &Ty) -> Option<Vec<String>> {
    match ty {
        Ty::Never | Ty::Lit(_, "string" | "number") => required_keys(ty),
        Ty::Union(parts) => parts.iter().try_fold(Vec::new(), |mut names, part| { names.extend(finite_keys(part)?); Some(names) }),
        _ => None,
    }
}

fn accepts_symbol(ty: &Ty) -> Option<bool> {
    match ty {
        Ty::Prim("symbol") => Some(true),
        Ty::Prim("string" | "number") | Ty::Lit(_, "string" | "number") | Ty::Never => Some(false),
        Ty::Union(parts) => {
            let mut unknown = false;
            for part in parts {
                match accepts_symbol(part) { Some(true) => return Some(true), None => unknown = true, _ => {} }
            }
            (!unknown).then_some(false)
        }
        _ => None,
    }
}

pub(super) fn required_keys(ty: &Ty) -> Option<Vec<String>> {
    match ty {
        Ty::Prim("string" | "number" | "symbol") | Ty::Never => Some(Vec::new()),
        Ty::Lit(text, "string") => Some(vec![super::super::regex::cook(text)?]),
        Ty::Lit(text, "number") => Some(vec![super::super::statics::number_string(super::super::statics::number(text)?)]),
        Ty::Union(parts) => {
            let strings = parts.iter().any(|part| matches!(part, Ty::Prim("string")));
            let numbers = parts.iter().any(|part| matches!(part, Ty::Prim("number")));
            parts.iter().try_fold(Vec::new(), |mut keys, part| {
                if !matches!(part, Ty::Lit(_, "string") if strings)
                    && !matches!(part, Ty::Lit(_, "number") if strings || numbers) {
                    keys.extend(required_keys(part)?);
                }
                Some(keys)
            })
        }
        _ => None,
    }
}

pub(super) fn accepts_key(ty: &Ty, key: &str) -> Option<bool> {
    match ty {
        Ty::Prim("string") => Some(true),
        Ty::Prim("number") => Some(super::super::statics::number(key).is_some_and(|n| super::super::statics::number_string(n) == key)),
        Ty::Prim("symbol") | Ty::Never => Some(false),
        Ty::Lit(_, "string" | "number") => Some(required_keys(ty)?.iter().any(|name| name == key)),
        Ty::Union(parts) => {
            let mut unknown = false;
            for part in parts {
                match accepts_key(part, key) {
                    Some(true) => return Some(true),
                    None => unknown = true,
                    _ => {}
                }
            }
            (!unknown).then_some(false)
        }
        _ => None,
    }
}

pub(super) fn keys_overlap(a: &Ty, b: &Ty) -> Option<bool> {
    match (a, b) {
        (Ty::Never, _) | (_, Ty::Never) => Some(false),
        (Ty::Union(parts), _) | (_, Ty::Union(parts)) => {
            let mut unknown = false;
            for part in parts {
                let answer = if matches!(a, Ty::Union(_)) { keys_overlap(part, b) } else { keys_overlap(a, part) };
                match answer {
                    Some(true) => return Some(true),
                    None => unknown = true,
                    _ => {}
                }
            }
            (!unknown).then_some(false)
        }
        (Ty::Prim(a @ ("string" | "number" | "symbol")), Ty::Prim(b @ ("string" | "number" | "symbol"))) =>
            Some(a == b || *a != "symbol" && *b != "symbol"),
        (Ty::Lit(_, "string" | "number"), _) => accepts_key(b, required_keys(a)?.first()?),
        (_, Ty::Lit(_, "string" | "number")) => keys_overlap(b, a),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::native_linter;
    use std::path::Path;

    const FILTER_TYPES: &str = "type Pick<T, K extends keyof T> = { [P in K]: T[P] }; type Omit<T, K extends keyof any> = Pick<T, Exclude<keyof T, K>>;";

    #[test]
    fn string_intrinsics_preserve_domains_and_transform_literal_unions() {
        for (name, input, values) in [
            ("Uppercase", "'aBc' | 'def'", "'ABC', 'DEF'"),
            ("Lowercase", "'aBc' | 'DEF'", "'abc', 'def'"),
            ("Capitalize", "'aBc' | ''", "'ABc', ''"),
            ("Uncapitalize", "'ABc' | ''", "'aBc', ''"),
            ("Capitalize", "'\\u{10428}BC' | '\\u{00df}abc'", "'\\u{10428}BC', 'SSabc'"),
        ] {
            let lint = native_linter(&format!("in ts_expression_statement.expression rule resolved {{ when count(type_literal_values) == 2 && all(type_literal_values, literal => contains([{values}], literal.value))\nwarn 'resolved' }}"));
            let code = format!("type {name}<S extends string> = intrinsic; declare const value: {name}<{input}>; value;");
            assert_eq!(lint.lint(Path::new("/root/lib.fixture.d.ts"), &code).len(), 1, "{code}");
        }
        let lint = native_linter("in ts_expression_statement.expression rule resolved { when type_is_string_like\nwarn 'resolved' }");
        let preserved = native_linter("in ts_expression_statement.expression rule preserved { when contains(default(type_flags, []), 'any') || contains(default(type_flags, []), 'never')\nwarn 'preserved' }");
        for name in ["Uppercase", "Lowercase", "Capitalize", "Uncapitalize"] {
            let code = format!("type {name}<S extends string> = intrinsic; declare const broad: {name}<string>; declare const loose: {name}<any>; declare const empty: {name}<never>; declare const missing: {name}<Missing>; broad; loose; empty; missing;");
            assert_eq!(lint.lint(Path::new("/root/lib.fixture.d.ts"), &code).len(), 1, "{code}");
            assert_eq!(preserved.lint(Path::new("/root/lib.fixture.d.ts"), &code).len(), 2, "{code}");
            let local = format!("type {name}<S> = number; declare const value: {name}<string>; value;");
            assert!(lint.lint(Path::new("/root/a.ts"), &local).is_empty());
        }
    }

    #[test]
    fn readonly_preserves_nested_property_types() {
        let linter = native_linter("in ts_expression_statement.expression rule unsafe-result { when contains(default(type_flags, []), 'any')\nwarn 'unsafe result' }");
        let code = "type Readonly<T> = { readonly [P in keyof T]: T[P] }; interface Shape { value: any; safe: number; } declare const wrapped: Readonly<Readonly<Shape>>; wrapped.value; wrapped.safe;";
        assert_eq!(linter.lint(Path::new("/root/lib.fixture.d.ts"), code).len(), 1);
        let code = "type Readonly<T> = { safe: number }; declare const wrapped: Readonly<{ value: any }>; wrapped.value; wrapped.safe;";
        assert!(linter.lint(Path::new("/root/a.ts"), code).is_empty());
    }

    #[test]
    fn mapped_key_filters_keep_inherited_generic_properties_and_methods() {
        let linter = native_linter("in ts_expression_statement.expression rule unsafe-result { when contains(default(type_flags, []), 'any')\nwarn 'unsafe result' }");
        let code = format!("{FILTER_TYPES} interface Base<T> {{ keep: T; drop: T; optional?: number; method(): T; }} interface Shape extends Base<any> {{}} declare const omitted: Omit<Shape, 'drop'>; declare const picked: Pick<Shape, 'keep'>; declare const nullable: Omit<Shape | null, 'keep'>; omitted.keep; omitted.drop; omitted.method(); picked.keep; picked.drop; nullable.drop;");
        assert_eq!(linter.lint(Path::new("/root/lib.fixture.d.ts"), &code).len(), 3);
    }

    #[test]
    fn mapped_key_filters_preserve_optional_properties_in_assignability() {
        let linter = native_linter("rule related-getter-setter-pairs");
        let code = format!("{FILTER_TYPES} interface Shape {{ required: number; optional?: number; }} declare class Optional {{ get value(): {{}}; set value(value: Pick<Shape, 'optional'>); }} declare class Required {{ get value(): {{}}; set value(value: Pick<Shape, 'required'>); }}");
        assert_eq!(linter.lint(Path::new("/root/lib.fixture.d.ts"), &code).len(), 1);
    }

    #[test]
    fn broad_keys_do_not_require_overridden_literal_properties() {
        let keys = Ty::Union(vec![Ty::Prim("string"), Ty::Lit("'required'".into(), "string")]);
        assert!(required_keys(&keys).unwrap().is_empty());
        let keys = Ty::Union(vec![Ty::Prim("number"), Ty::Lit("1".into(), "number"), Ty::Lit("'named'".into(), "string")]);
        assert_eq!(required_keys(&keys).unwrap().len(), 1);
        assert_eq!(accepts_key(&keys, "2"), Some(true));
        assert_eq!(accepts_key(&keys, "01"), Some(false));
        assert_eq!(accepts_key(&keys, "unrelated"), Some(false));
    }
}
