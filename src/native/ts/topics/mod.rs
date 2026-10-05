//! Rule families' own native analyses and facts, one module each. A family
//! adds facts to its module's `FACTS` list (read through
//! [`crate::native::ts::facts::Facts::with_file`]) and keeps whole-file
//! results with [`crate::native::ts::facts::NativeFile::cached`].

use crate::expr::facts::{Fact, Link};

pub mod conditions;
pub mod jsx;
pub mod layout;
pub mod relations;
#[cfg(test)]
pub mod restrictions;
pub mod scoping;
pub mod structure;
pub mod suggestions;
pub mod syntax;
pub mod typed;

/// Every family's facts (after the core list in `expr::facts`).
pub const FACT_GROUPS: &[&[Fact]] = &[syntax::FACTS, layout::FACTS, structure::FACTS, typed::FACTS, conditions::FACTS, scoping::FACTS, suggestions::FACTS, jsx::FACTS];

/// Every link between nodes.
pub const LINK_GROUPS: &[&[Link]] = &[relations::LINKS];
