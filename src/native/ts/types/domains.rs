//! Possible runtime value domains, independent of conditional rule policy.

use super::{Checker, Ty};

#[derive(Clone, Copy, Debug)]
pub struct ValueDomains {
    pub truthy: bool,
    pub falsy: bool,
    pub nullish: bool,
    pub non_nullish: bool,
    pub opaque: bool,
}

#[derive(Clone, Debug)]
pub enum LiteralValue {
    String(String),
    Number(f64),
    Boolean(bool),
    BigInt(Box<str>),
    Null,
    Undefined,
}

impl LiteralValue {
    fn falsy(&self) -> Option<bool> {
        Some(match self {
            Self::String(value) => value.is_empty(),
            Self::Number(value) => *value == 0.0 || value.is_nan(),
            Self::Boolean(value) => !value,
            Self::BigInt(value) => literal_number(value.strip_suffix('n')?).map(|value| value == 0.0)?,
            Self::Null | Self::Undefined => true,
        })
    }
}

fn literal_number(text: &str) -> Option<f64> {
    if let Some(value) = text.strip_prefix('-') {
        super::super::statics::number(value).map(|value| -value)
    } else {
        super::super::statics::number(text.trim_start_matches('+'))
    }
}

fn primitive_domain(name: &str) -> u8 {
    match name {
        "string" => 1,
        "number" => 2,
        "boolean" => 4,
        "bigint" => 8,
        "symbol" => 16,
        _ => u8::MAX,
    }
}

impl Checker<'_> {
    pub fn literal_value(&self, ty: &Ty) -> Option<LiteralValue> {
        if let Some(source) = self.readonly_source(ty) { return self.literal_value(source) }
        Some(match ty {
            Ty::Lit(value, "string") => LiteralValue::String(super::super::regex::cook(value)?),
            Ty::Lit(value, "number") => LiteralValue::Number(literal_number(value)?),
            Ty::Lit(value, "boolean") if &**value == "true" || &**value == "false" => LiteralValue::Boolean(&**value == "true"),
            Ty::Lit(value, "bigint") => LiteralValue::BigInt(value.clone()),
            Ty::Null => LiteralValue::Null,
            Ty::Undefined => LiteralValue::Undefined,
            _ => return None,
        })
    }

    pub fn literal_falsy(&self, ty: &Ty) -> Option<bool> {
        if let Some(source) = self.readonly_source(ty) { return self.literal_falsy(source) }
        if let Some(value) = self.literal_value(ty) { return value.falsy() }
        match ty {
            Ty::Void => Some(true),
            Ty::Prim("string" | "number" | "boolean" | "bigint") => None,
            Ty::Named(symbols, _) if self.is_enum(symbols) => None,
            Ty::Named(..) | Ty::Array(_) | Ty::Tuple(_) | Ty::Obj(..) | Ty::Struct(_) | Ty::Ns(_) | Ty::Module(_) | Ty::Fn(_) | Ty::Class(_) | Ty::Prim(_) => Some(false),
            _ => None,
        }
    }

    pub fn value_domains(&self, ty: &Ty) -> ValueDomains {
        if let Some(source) = self.readonly_source(ty) { return self.value_domains(source) }
        if let Ty::Union(types) | Ty::Inter(types) = ty {
            let parts: Vec<_> = types.iter().map(|part| self.value_domains(part)).collect();
            let empty = self.primitive_domains(ty) == 0;
            let possible = |read: fn(&ValueDomains) -> bool| {
                !empty && if matches!(ty, Ty::Union(_)) { parts.iter().any(read) } else { parts.iter().all(read) }
            };
            let falsy = if matches!(ty, Ty::Union(_)) {
                parts.iter().any(|part| part.falsy)
            } else {
                // Brands do not remove falsy primitive values; literal
                // constraints do, even when nested inside unions.
                let mut constraints = types.iter().zip(&parts).filter(|(ty, _)| self.restricts_falsy(ty)).map(|(_, domain)| domain.falsy);
                constraints.next().map_or_else(|| parts.iter().any(|part| part.falsy), |first| first && constraints.all(|falsy| falsy))
            };
            return ValueDomains {
                truthy: possible(|part| part.truthy),
                falsy: !empty && falsy,
                nullish: possible(|part| part.nullish),
                non_nullish: possible(|part| part.non_nullish),
                opaque: parts.iter().any(|part| part.opaque),
            };
        }
        let unknown = matches!(ty, Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_));
        let never = matches!(ty, Ty::Never);
        let nullish = matches!(ty, Ty::Null | Ty::Undefined | Ty::Void);
        let falsy = self.literal_falsy(ty);
        ValueDomains {
            truthy: !never && falsy != Some(true),
            falsy: !never && falsy != Some(false),
            nullish: nullish || unknown,
            non_nullish: !never && (!nullish || unknown),
            opaque: matches!(ty, Ty::Unknown) || matches!(ty, Ty::Lit(..)) && falsy.is_none(),
        }
    }

    fn primitive_domains(&self, ty: &Ty) -> u8 {
        if let Some(source) = self.readonly_source(ty) { return self.primitive_domains(source) }
        match ty {
            Ty::Prim(name) | Ty::Lit(_, name) => primitive_domain(name),
            Ty::Null => 32,
            Ty::Undefined | Ty::Void => 64,
            Ty::Never => 0,
            Ty::Union(parts) => parts.iter().fold(0, |mask, part| mask | self.primitive_domains(part)),
            Ty::Inter(parts) => parts.iter().fold(u8::MAX, |mask, part| mask & self.primitive_domains(part)),
            _ => u8::MAX,
        }
    }

    fn restricts_falsy(&self, ty: &Ty) -> bool {
        if let Some(source) = self.readonly_source(ty) { return self.restricts_falsy(source) }
        match ty {
            Ty::Prim("string" | "number" | "boolean" | "bigint" | "symbol") | Ty::Lit(..) | Ty::Null | Ty::Undefined | Ty::Void | Ty::Never => true,
            Ty::Named(symbols, _) => self.is_enum(symbols),
            Ty::Union(parts) => parts.iter().all(|part| self.restricts_falsy(part)),
            Ty::Inter(parts) => parts.iter().any(|part| self.restricts_falsy(part)),
            _ => false,
        }
    }
}
