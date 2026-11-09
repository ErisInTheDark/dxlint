//! Metadata and value callback of a built-in expression function.

use crate::value::Value;

pub struct Function {
    pub name: &'static str,
    pub signature: &'static str,
    pub doc: &'static str,
    pub min: usize,
    pub max: usize,
    pub call: for<'s> fn(&[Value<'s>]) -> Result<Value<'s>, String>,
}

impl std::fmt::Debug for Function {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.signature)
    }
}
