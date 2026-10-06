//! Parameter positions shared by call-context and transfer queries.

use super::Ty;

pub struct Parameters {
    types: Vec<Ty>,
    rest: bool,
    at: usize,
    consumed: bool,
}

impl Parameters {
    pub fn new(types: Vec<Ty>, rest: bool, at: usize) -> Self { Self { types, rest, at, consumed: false } }

    /// A resolved erroneous call retains its final rest slot for recovery.
    pub fn next_recovery(&mut self) -> Option<Ty> { self.advance(true) }

    pub fn consume_remaining(&mut self) { self.consumed = true; }

    fn advance(&mut self, recovery: bool) -> Option<Ty> {
        let at = self.at;
        self.at += 1;
        let rest_at = self.types.len().saturating_sub(1);
        if self.rest && (at >= rest_at || self.consumed) {
            match self.types.last()? {
                Ty::Array(element) => Some((*element.element).clone()),
                Ty::Tuple(elements) => {
                    let index = if self.consumed { elements.len().saturating_sub(1) } else { at.saturating_sub(rest_at) };
                    let mut ty = elements.at(index);
                    if recovery && ty.is_opaque() {
                        ty = elements.at(elements.len().saturating_sub(1));
                    }
                    (!ty.is_opaque()).then_some(ty)
                }
                ty if recovery => Some(ty.clone()),
                _ => None,
            }
        } else if self.consumed {
            None
        } else {
            self.types.get(at).cloned()
        }
    }
}

impl Iterator for Parameters {
    type Item = Ty;

    /// Strict declared positions; extra fixed-tuple arguments have no slot.
    fn next(&mut self) -> Option<Ty> { self.advance(false) }
}
