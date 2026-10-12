use super::Name;
use crate::config::source::Span;

/// Traits that cannot occur together in one possible value.
#[derive(Clone, Debug)]
pub struct Exclusive {
    pub traits: Vec<Name>,
    pub span: Span,
}
