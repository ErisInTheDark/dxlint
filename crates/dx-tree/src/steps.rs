//! Path steps that are not field labels.

/// Path steps that are not fields: the node's parent (`parent.parameter`),
/// the node something unwraps from (`it`), the reference a node was reached
/// through (`through`), and a name's declaration.
pub const PARENT: u16 = u16::MAX;
pub const IT: u16 = u16::MAX - 1;
pub const THROUGH: u16 = u16::MAX - 2;
pub const DECLARATION: u16 = u16::MAX - 3;
/// The node's index among its parent's nodes in the same field (a type
/// parameter's position, for `from through.type_argument at position`).
pub const POSITION: u16 = u16::MAX - 4;
pub const RESULT: u16 = u16::MAX - 5;
pub const EXPECTED: u16 = u16::MAX - 6;
pub const INFERRED: u16 = u16::MAX - 7;
pub const RETURNED: u16 = u16::MAX - 8;
pub const WIDENED: u16 = u16::MAX - 9;
pub const RECEIVER: u16 = u16::MAX - 10;
pub const PRODUCT_ITEMS: u16 = u16::MAX - 11;
pub const FIRST_PARAMETER: u16 = u16::MAX - 12;
/// The lowest special step: attribute steps stay below it.
pub const SPECIAL_STEPS: u16 = FIRST_PARAMETER;
/// `ATTR_STEP | id`: the node a `let` attribute (by engine attribute id)
/// holds; fields are numbered below it, the special steps above.
pub const ATTR_STEP: u16 = 0x8000;
