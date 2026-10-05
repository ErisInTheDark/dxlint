use super::{Expr, Operand, Operator, Relation};

pub(super) fn parse(source: &str) -> Result<Expr, String> {
    if source.len() > 65536 { return Err("selector exceeds 64 KiB".into()); }
    let mut parser = Parser { source, at: 0, depth: 0 };
    parser.space();
    let expr = parser.list()?;
    parser.space();
    if parser.at != source.len() { return Err(parser.error("unexpected selector token")); }
    Ok(expr)
}

struct Parser<'a> { source: &'a str, at: usize, depth: usize }

impl Parser<'_> {
    fn error(&self, message: &str) -> String {
        format!("invalid selector at byte {}: {message}", self.at)
    }
    fn peek(&self) -> Option<char> { self.source[self.at..].chars().next() }
    fn take(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += c.len_utf8();
        Some(c)
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() != Some(c) { return false; }
        self.take();
        true
    }
    fn expect(&mut self, c: char) -> Result<(), String> {
        if self.eat(c) { Ok(()) } else { Err(self.error(&format!("expected {c}"))) }
    }
    fn space(&mut self) -> bool {
        let before = self.at;
        while self.peek().is_some_and(char::is_whitespace) { self.take(); }
        self.at != before
    }
    fn word(&mut self) -> Result<String, String> {
        let start = self.at;
        while self.peek().is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '-')) { self.take(); }
        if self.at == start { return Err(self.error("expected name")); }
        Ok(self.source[start..self.at].to_owned())
    }
    fn path(&mut self) -> Result<Vec<String>, String> {
        let mut path = vec![self.word()?];
        while self.eat('.') { path.push(self.word()?); }
        Ok(path)
    }
    fn list(&mut self) -> Result<Expr, String> {
        self.depth += 1;
        if self.depth > 128 { return Err(self.error("selector nesting exceeds 128")); }
        let mut parts = vec![self.sequence()?];
        while self.eat(',') { self.space(); parts.push(self.sequence()?); }
        self.depth -= 1;
        Ok(if parts.len() == 1 { parts.remove(0) } else { Expr::Any(parts) })
    }
    fn relation(&mut self) -> Option<Relation> {
        let relation = match self.peek()? {
            '>' => Relation::Child, '+' => Relation::Adjacent, '~' => Relation::Sibling, _ => return None,
        };
        self.take();
        Some(relation)
    }
    fn sequence(&mut self) -> Result<Expr, String> {
        let mut length = 1;
        let mut left = if let Some(relation) = self.relation() {
            self.space();
            Expr::Join(Box::new(Expr::Root), relation, Box::new(self.compound()?))
        } else { self.compound()? };
        loop {
            let space = self.space();
            if matches!(self.peek(), None | Some(',' | ')')) { break; }
            let relation = match self.relation() {
                Some(relation) => { self.space(); relation }
                None if space => Relation::Descendant,
                None => return Err(self.error("expected combinator")),
            };
            length += 1;
            if length > 128 { return Err(self.error("selector chain exceeds 128")); }
            left = Expr::Join(Box::new(left), relation, Box::new(self.compound()?));
        }
        Ok(left)
    }
    fn compound(&mut self) -> Result<Expr, String> {
        let mut parts = Vec::new();
        loop {
            let expr = match self.peek() {
                Some('*') => { self.take(); Expr::All(vec![]) }
                Some('#') => { self.take(); Expr::Type(self.word()?) }
                Some('.') => { self.take(); Expr::Field(self.path()?) }
                Some('[') => self.attribute()?,
                Some(':') => self.pseudo()?,
                Some('!') => return Err(self.error("subject selectors are unsupported")),
                Some(c) if c.is_alphanumeric() || c == '_' => Expr::Type(self.word()?),
                _ => break,
            };
            parts.push(expr);
        }
        if parts.is_empty() { return Err(self.error("expected selector")); }
        Ok(if parts.len() == 1 { parts.remove(0) } else { Expr::All(parts) })
    }
    fn pseudo(&mut self) -> Result<Expr, String> {
        self.expect(':')?;
        let name = self.word()?.to_ascii_lowercase();
        match name.as_str() {
            "not" | "is" | "matches" | "has" => {
                self.expect('(')?;
                self.space();
                let expr = self.list()?;
                self.expect(')')?;
                Ok(match name.as_str() { "not" => Expr::Not(Box::new(expr)), "has" => Expr::Has(Box::new(expr)), _ => expr })
            }
            "first-child" => Ok(Expr::Nth(1, false)),
            "last-child" => Ok(Expr::Nth(1, true)),
            "nth-child" | "nth-last-child" => {
                self.expect('(')?;
                self.space();
                let start = self.at;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) { self.take(); }
                let n = self.source[start..self.at].parse().map_err(|_| self.error("expected nonnegative child index"))?;
                self.space();
                self.expect(')')?;
                Ok(Expr::Nth(n, name == "nth-last-child"))
            }
            "statement" | "declaration" | "expression" | "pattern" | "function" => Ok(Expr::Class(name)),
            _ => Err(self.error("unknown selector class")),
        }
    }
    fn attribute(&mut self) -> Result<Expr, String> {
        self.expect('[')?;
        self.space();
        let path = self.path()?;
        self.space();
        if self.eat(']') { return Ok(Expr::Attribute(path, None)); }
        let op = match self.take() {
            Some('=') => Operator::Equal,
            Some('!') => { self.expect('=')?; Operator::Unequal }
            Some('<') => if self.eat('=') { Operator::LessEqual } else { Operator::Less },
            Some('>') => if self.eat('=') { Operator::GreaterEqual } else { Operator::Greater },
            _ => return Err(self.error("expected attribute operator")),
        };
        self.space();
        let value = if self.eat('/') {
            let mut pattern = String::new();
            let mut class = false;
            loop {
                match self.take() {
                    None => return Err(self.error("unterminated regex")),
                    Some('/') if !class => break,
                    Some('\\') => {
                        pattern.push('\\');
                        pattern.push(self.take().ok_or_else(|| self.error("unterminated regex escape"))?);
                    }
                    Some(c) => { if c == '[' { class = true; } else if c == ']' { class = false; } pattern.push(c); }
                }
            }
            let start = self.at;
            while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) { self.take(); }
            Operand::Regex(crate::js_regex::Regex::compile(&pattern, &self.source[start..self.at])?)
        } else if self.source[self.at..].starts_with("type(") {
            self.at += 5;
            let value = self.word()?;
            self.expect(')')?;
            Operand::Type(value)
        } else if matches!(self.peek(), Some('"' | '\'')) {
            Operand::Literal(self.quoted()?, false)
        } else {
            let start = self.at;
            while self.peek().is_some_and(|c| c != ']' && !c.is_whitespace()) { self.take(); }
            if self.at == start { return Err(self.error("expected attribute value")); }
            let text = self.source[start..self.at].to_owned();
            let numeric = text.starts_with(|c: char| c.is_ascii_digit() || c == '.');
            let text = if numeric {
                if text.ends_with('.') || text.chars().any(|c| !c.is_ascii_digit() && c != '.') { return Err(self.error("invalid numeric attribute literal")); }
                crate::expr::value::js_number_text(text.parse::<f64>().map_err(|_| self.error("invalid numeric attribute literal"))?)
            } else {
                if text.contains(['!', '#', '(', ')', '+', ',', '.', ':', '<', '=', '>', '[', '~']) {
                    return Err(self.error("invalid unquoted attribute literal"));
                }
                text
            };
            Operand::Literal(text, numeric)
        };
        self.space();
        self.expect(']')?;
        Ok(Expr::Attribute(path, Some((op, value))))
    }
    fn quoted(&mut self) -> Result<String, String> {
        let quote = self.take().ok_or_else(|| self.error("expected string"))?;
        let mut text = String::new();
        loop {
            match self.take() {
                Some(c) if c == quote => return Ok(text),
                Some('\\') => {
                    let c = self.take().ok_or_else(|| self.error("unterminated escape"))?;
                    text.push(match c { 'n' => '\n', 'r' => '\r', 't' => '\t', 'b' => '\u{8}', 'f' => '\u{c}', 'v' => '\u{b}', _ => c });
                }
                Some(c) => text.push(c),
                None => return Err(self.error("unterminated string")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn excessive_selector_chains_fail_before_matching() {
        assert!(super::parse(&vec!["Identifier"; 129].join(" > ")).is_err());
        assert!(super::parse(&vec!["Identifier"; 128].join(" > ")).is_ok());
    }
}
