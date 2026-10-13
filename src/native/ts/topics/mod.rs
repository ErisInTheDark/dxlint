//! Rule families' own native analyses and facts, one module each. A family
//! adds facts to its module's `FACTS` list (read through
//! [`crate::native::ts::facts::Facts::with_file`]) and keeps whole-file
//! results with [`crate::native::ts::facts::NativeFile::cached`]. The
//! front end hands every list to the engine
//! ([`crate::engine::frontend::Frontend::facts`]).

use std::sync::LazyLock;

use crate::expr::facts::Shape;
use crate::expr::Value;

use super::facts::Facts;

pub mod conditions;
pub mod core;
pub mod layout;
pub mod structure;
pub mod suggestions;
pub mod syntax;
pub mod symbols;
pub mod type_relations;
pub mod typed;

/// A fact. Topic modules read the file directly through
/// [`Facts::with_file`]: `get: |f, n| f.with_file(my_fact, n)` with
/// `fn my_fact(file: &NativeFile, node: u32) -> Value<'static>`.
pub struct Fact {
    pub name: &'static str,
    pub doc: &'static str,
    pub get: fn(&dyn Facts, u32) -> Value<'static>,
    pub shape: Shape,
}

impl Fact {
    pub const SCALAR: Self = Self { name: "", doc: "", get: |_, _| Value::Null, shape: Shape::Scalar };
}

/// Optional projections of a record-or-null fact. `None` asks whether the
/// fact is present; field reads must agree with the eager record.
pub struct FactFields {
    pub name: &'static str,
    pub fields: &'static [&'static str],
    pub get: fn(&dyn Facts, u32, Option<usize>) -> Value<'static>,
}

/// A link: nodes a node leads to (see [`crate::expr::facts::LinkInfo`]).
pub struct Link {
    pub name: &'static str,
    pub doc: &'static str,
    pub many: bool,
    pub get: fn(&dyn Facts, u32) -> Vec<u32>,
}

const FACT_GROUPS: &[&[Fact]] = &[
    core::FACTS, syntax::FACTS, layout::FACTS, structure::FACTS, typed::FACTS, conditions::FACTS,
    type_relations::FACTS, suggestions::FACTS, symbols::FACTS,
];

const FACT_FIELD_GROUPS: &[&[FactFields]] = &[syntax::FACT_FIELDS];

const LINK_GROUPS: &[&[Link]] = &[layout::LINKS];

/// Every fact, numbered as the engine reads them.
pub static FACTS: LazyLock<Vec<&'static Fact>> = LazyLock::new(|| FACT_GROUPS.iter().flat_map(|group| group.iter()).collect());

/// Every projection, numbered as the engine reads them.
pub static PROJECTIONS: LazyLock<Vec<&'static FactFields>> = LazyLock::new(|| FACT_FIELD_GROUPS.iter().flat_map(|group| group.iter()).collect());

/// Every link, numbered as the engine reads them.
pub static LINKS: LazyLock<Vec<&'static Link>> = LazyLock::new(|| LINK_GROUPS.iter().flat_map(|group| group.iter()).collect());
