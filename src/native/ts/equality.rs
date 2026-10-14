//! Code compared the way eslint's `equalTokens` compares it: the same
//! tokens, whatever the spacing and comments between them.

use super::facts::NativeFile;

impl NativeFile<'_> {
    /// eslint's `equalTokens` (outer parentheses aside).
    pub fn equal_tokens(&self, a: u32, b: u32) -> bool {
        self.tokens(self.syntax.unparen(a)) == self.tokens(self.syntax.unparen(b))
    }
}
