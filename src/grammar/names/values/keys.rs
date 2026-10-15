use super::{Identity, Key, Known, Leaf, NodeValue, Values};
use crate::config::ast::values::KeyDomain;

impl Key {
    pub(super) fn ordinal(&self) -> Option<u32> {
        match self {
            Self::Ordinal(index) => Some(*index),
            Self::Text(text) => text.parse::<u32>().ok().filter(|index| index.to_string() == &**text),
            _ => None,
        }
    }

    pub fn domain(&self) -> KeyDomain {
        match self {
            Self::Text(_) => KeyDomain::Text,
            Self::Ordinal(_) => KeyDomain::Ordinal,
            Self::Symbol(_) => KeyDomain::Symbol,
            Self::Domain(domain) => *domain,
        }
    }
}

impl Values<'_> {
    fn key_domain(&self, leaf: Leaf) -> Known<Option<KeyDomain>> {
        let mut answer = None;
        let mut unknown = false;
        for &(domain, traits) in &self.tables.key_domains {
            for id in 0..64 {
                if traits & (1 << id) == 0 { continue }
                match self.fits(leaf, id) {
                    Known::Yes(true) => {
                        if answer.is_some_and(|previous| previous != domain) { return Known::Unknown }
                        answer = Some(domain);
                    },
                    Known::Unknown => unknown = true,
                    Known::Yes(false) => {},
                }
            }
        }
        if answer.is_none() && unknown { Known::Unknown } else { Known::Yes(answer) }
    }

    /// The one key a key expression evaluates to: several literal keys of
    /// one domain widen to that domain.
    pub fn value_key(&self, value: &NodeValue) -> Known<Key> {
        let Known::Yes(keys) = self.value_keys(value) else { return Known::Unknown };
        let mut answer: Option<Key> = None;
        for key in keys {
            answer = match answer {
                None => Some(key),
                Some(previous) if previous.domain() == key.domain() => Some(Key::Domain(key.domain())),
                Some(_) => return Known::Unknown,
            };
        }
        answer.map_or(Known::Unknown, Known::Yes)
    }

    /// The distinct keys a key expression's alternatives evaluate to, in
    /// order: each literal of a finite union (`'a' | 'b'`), or a wide domain.
    pub fn value_keys(&self, value: &NodeValue) -> Known<Vec<Key>> {
        let NodeValue::Alternatives(rows) = value else { return Known::Unknown };
        let mut keys: Vec<Key> = Vec::new();
        for row in rows {
            let mut literal = None;
            let mut domain = None;
            for &leaf in row {
                if !self.question.budget.step() { return Known::Unknown }
                let key = match self.identity(leaf) {
                    Known::Yes(Some(Identity::Value(value))) => Self::expression_key(value),
                    Known::Yes(Some(Identity::Declaration(node))) => {
                        if self.key_domain(leaf) != Known::Yes(Some(KeyDomain::Symbol)) { return Known::Unknown }
                        Some(Key::Symbol(node))
                    },
                    Known::Yes(None) => {
                        match self.key_domain(leaf) {
                            Known::Yes(Some(current)) => {
                                if domain.is_some_and(|previous| previous != current) { return Known::Unknown }
                                domain = Some(current);
                            },
                            Known::Yes(None) => {},
                            Known::Unknown => return Known::Unknown,
                        }
                        None
                    },
                    Known::Unknown => return Known::Unknown,
                };
                if let Some(key) = key {
                    if literal.as_ref().is_some_and(|previous| previous != &key) { return Known::Unknown }
                    literal = Some(key);
                }
            }
            let Some(key) = literal.or_else(|| domain.map(Key::Domain)) else { return Known::Unknown };
            if !keys.contains(&key) { keys.push(key); }
        }
        if keys.is_empty() { Known::Unknown } else { Known::Yes(keys) }
    }
}
