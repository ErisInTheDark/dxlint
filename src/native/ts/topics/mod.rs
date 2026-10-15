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

pub mod core;
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

const FACT_GROUPS: &[&[Fact]] = &[
    core::FACTS, typed::FACTS, type_relations::FACTS, symbols::FACTS,
];

/// Every fact, numbered as the engine reads them.
pub static FACTS: LazyLock<Vec<&'static Fact>> = LazyLock::new(|| FACT_GROUPS.iter().flat_map(|group| group.iter()).collect());
