use super::{Context, Parser};

impl Parser<'_> {
    pub(super) fn exclusive_item(&mut self, _context: Option<&Context>) {
        let keyword = self.bump().expect("peeked");
        self.errors.push(crate::config::error::ConfigError::new(keyword.span, "exclusive groups are not implemented"));
    }
}
