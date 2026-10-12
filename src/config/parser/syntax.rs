//! Structured grammar declarations beside byte-pattern declarations.

use super::Parser;

impl Parser<'_> {
    pub(super) fn syntax_parser(&mut self) {
        self.bump();
        self.error_here("structured grammar declarations are not implemented yet");
    }
}
