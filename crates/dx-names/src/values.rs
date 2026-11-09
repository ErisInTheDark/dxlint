//! Values: the recipes and keys `unwraps`, `fits`, `could` / `always` and
//! `through` generics are declared with. The evaluation over them lives in
//! `dx-semantic`.

use super::question::{Known, Leaf, NodeRef, NodeValue};
use super::Value;

pub mod compile;
pub mod products;

pub(super) fn algebra_traits(
    traits: &[dx_config::ast::Name],
    vocabulary: &super::Vocabulary,
    names: &super::Names,
    errors: &mut Vec<dx_config::error::ConfigError>,
) -> u64 {
    let fitted = fitted_traits(names);
    let mut mask = 0;
    for name in traits {
        let bit = vocabulary.trait_bit(&name.text);
        if fitted & bit == 0 {
            errors.push(dx_config::error::ConfigError::new(name.span, format!("nothing fits trait `{}`", name.text)));
        } else {
            mask |= bit;
        }
    }
    mask
}

pub(super) fn exclusive_groups(
    config: &dx_config::ast::Config,
    vocabulary: &mut super::Vocabulary,
    names: &super::Names,
    errors: &mut Vec<dx_config::error::ConfigError>,
) -> Vec<u64> {
    use dx_config::error::ConfigError;
    let fitted = fitted_traits(names);
    let mut groups = Vec::new();
    for group in &config.exclusive {
        let Some(bits) = super::class_bits(&mut vocabulary.traits, &group.traits, errors) else { continue };
        let mut valid = true;
        for name in &group.traits {
            let bit = vocabulary.traits.iter().position(|trait_name| trait_name == &name.text).map_or(0, |index| 1 << index);
            if fitted & bit == 0 {
                errors.push(ConfigError::new(name.span, format!("nothing fits trait `{}`", name.text))
                    .suggest(&name.text, vocabulary.traits.iter().enumerate()
                        .filter(|(index, _)| fitted & (1 << index) != 0)
                        .map(|(_, name)| name.as_str())));
                valid = false;
            }
        }
        if bits.count_ones() < 2 {
            errors.push(ConfigError::new(group.span, "`exclusive` needs at least two distinct traits"));
            valid = false;
        }
        if valid { groups.push(bits) }
    }
    groups
}

fn fitted_traits(names: &super::Names) -> u64 {
    let ordinary = names.kinds.iter().flat_map(|kind| &kind.fits).fold(0, |all, &(bits, _, _)| all | bits);
    names.products.iter().flatten().fold(ordinary, |all, shape| all | shape.traits)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Text(Box<str>),
    Ordinal(u32),
    Symbol(NodeRef),
    Domain(dx_config::ast::values::KeyDomain),
}

impl Key {
    pub fn may_alias(&self, other: &Self) -> bool {
        self == other || matches!((self, other),
            (Self::Text(_), Self::Ordinal(_)) | (Self::Ordinal(_), Self::Text(_)))
            || (matches!(self, Self::Domain(_)) || matches!(other, Self::Domain(_)))
                && (self.domain() == other.domain()
                    || matches!((self.domain(), other.domain()),
                        (dx_config::ast::values::KeyDomain::Text, dx_config::ast::values::KeyDomain::Ordinal)
                        | (dx_config::ast::values::KeyDomain::Ordinal, dx_config::ast::values::KeyDomain::Text)))
    }

    pub fn ordinal(&self) -> Option<u32> {
        match self {
            Self::Ordinal(index) => Some(*index),
            Self::Text(text) => text.parse::<u32>().ok().filter(|index| index.to_string() == &**text),
            _ => None,
        }
    }

    pub fn domain(&self) -> dx_config::ast::values::KeyDomain {
        match self {
            Self::Text(_) => dx_config::ast::values::KeyDomain::Text,
            Self::Ordinal(_) => dx_config::ast::values::KeyDomain::Ordinal,
            Self::Symbol(_) => dx_config::ast::values::KeyDomain::Symbol,
            Self::Domain(domain) => *domain,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Identity {
    Value(dx_ir::value::Value<'static>),
    Declaration(NodeRef),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub key: Key,
    pub sources: Vec<MemberSource>,
    pub required: Known<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberSource {
    pub site: Leaf,
    pub value: Option<Value>,
    pub origin: MemberOrigin,
    /// Declaration flags changed by the modified value it was read through.
    pub edits: Option<super::question::ModifiedId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberOrigin {
    Named,
    Ordinal { repeated: bool },
    Indexer(dx_config::ast::values::KeyDomain),
}

impl MemberSource {
    pub fn normal(site: Leaf) -> Self {
        Self { site, value: None, origin: MemberOrigin::Named, edits: None }
    }
}

#[derive(Clone, Debug)]
pub struct CarrierRecipe {
    pub description: u32,
    pub arguments: Option<Vec<u16>>,
    pub value: Option<Value>,
    pub when: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct IndexedRecipe {
    pub path: Vec<u16>,
    pub mode: IndexedMode,
    pub views: u64,
    pub value: Option<Value>,
    pub when: Option<u32>,
    /// A runtime member read that is no key of the type: `keys of` skips it.
    pub runtime: bool,
}

#[derive(Clone, Debug)]
pub enum IndexedMode {
    Slots { repeated: bool, strings: bool },
    Domain(dx_config::ast::values::KeyDomain),
    Keyed(Value),
    Keys { bind: Option<Vec<u16>> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Carrier {
    pub declaration: NodeRef,
    pub description: dx_ir::value::Value<'static>,
    pub arguments: Vec<NodeValue>,
}
