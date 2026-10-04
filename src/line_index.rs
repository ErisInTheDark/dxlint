//! Byte offset <-> line/column conversion, shared by the CLI renderer, the
//! rule engine (`line`/`col` builtins) and the LSP (UTF-16 positions).

#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of the first byte of every line. Always starts with 0.
    starts: Vec<u32>,
    len: u32,
}

impl LineIndex {
    /// Lines end at `\n`, `\r\n` or a lone `\r` (as LSP and eslint count).
    pub fn new(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut starts = Vec::with_capacity(text.len() / 32 + 1);
        starts.push(0);
        starts.extend(
            memchr::memchr2_iter(b'\n', b'\r', bytes)
                .filter(|&i| !(bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n')))
                .map(|i| i as u32 + 1),
        );
        LineIndex { starts, len: text.len() as u32 }
    }

    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    /// Zero-based line containing `offset`.
    pub fn line_of(&self, offset: usize) -> usize {
        match self.starts.binary_search(&(offset as u32)) {
            Ok(line) => line,
            Err(next) => next - 1,
        }
    }

    pub fn line_start(&self, line: usize) -> usize {
        self.starts.get(line).copied().unwrap_or(self.len) as usize
    }

    /// End of the line's content, excluding `\n` / `\r\n` / `\r`.
    pub fn line_end(&self, text: &str, line: usize) -> usize {
        let next = self.starts.get(line + 1).map(|&s| s as usize).unwrap_or(text.len());
        let bytes = text.as_bytes();
        let mut end = next;
        if end > self.line_start(line) && bytes.get(end - 1) == Some(&b'\n') {
            end -= 1;
        }
        if end > self.line_start(line) && bytes.get(end - 1) == Some(&b'\r') {
            end -= 1;
        }
        end
    }

    pub fn line_text<'t>(&self, text: &'t str, line: usize) -> &'t str {
        &text[self.line_start(line)..self.line_end(text, line)]
    }

    /// Zero-based (line, column in chars).
    pub fn line_col(&self, text: &str, offset: usize) -> (usize, usize) {
        let line = self.line_of(offset);
        let start = self.line_start(line);
        (line, text[start..offset.min(text.len())].chars().count())
    }

    /// Zero-based (line, column in UTF-16 code units) as used by LSP.
    pub fn to_utf16(&self, text: &str, offset: usize) -> (u32, u32) {
        let line = self.line_of(offset);
        let start = self.line_start(line);
        let col: usize = text[start..offset.min(text.len())].chars().map(char::len_utf16).sum();
        (line as u32, col as u32)
    }

    /// Inverse of [`LineIndex::to_utf16`]; clamps out-of-range positions.
    pub fn from_utf16(&self, text: &str, line: u32, col: u32) -> usize {
        let line = line as usize;
        if line >= self.starts.len() {
            return text.len();
        }
        let start = self.line_start(line);
        let end = self.line_end(text, line);
        let mut units = 0u32;
        for (i, c) in text[start..end].char_indices() {
            if units >= col {
                return start + i;
            }
            units += c.len_utf16() as u32;
        }
        end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_round_trips_through_astral_chars() {
        let text = "a\r\nx😀y\nlast";
        let idx = LineIndex::new(text);
        let y = text.find('y').unwrap();
        assert_eq!(idx.to_utf16(text, y), (1, 3));
        assert_eq!(idx.from_utf16(text, 1, 3), y);
        assert_eq!(idx.line_text(text, 0), "a");
        assert_eq!(idx.line_col(text, y), (1, 2));
        assert_eq!(idx.from_utf16(text, 9, 0), text.len());
    }

    #[test]
    fn a_lone_carriage_return_ends_a_line() {
        let text = "a\rb\r\nc\nd";
        let idx = LineIndex::new(text);
        assert_eq!(idx.line_count(), 4);
        assert_eq!(idx.line_col(text, text.find('c').unwrap()), (2, 0));
        assert_eq!((idx.line_text(text, 0), idx.line_text(text, 1)), ("a", "b"));
    }
}
