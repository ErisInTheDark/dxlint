use super::{Identity, Key, Known, Leaf, NodeValue, Values};
use crate::grammar::names::question::KeyId;
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
    /// A key fits exactly its domain's traits, as a node fits what it
    /// declares, and a literal key the truthiness trait of its identity
    /// (`keys truthiness`). Only an exclusive group naming none of those (a
    /// domain key's truthiness) stays undecided.
    pub(super) fn key_trait(&self, id: KeyId, trait_id: u16) -> Known<bool> {
        let Some(key) = self.question.key(id) else { return Known::Unknown };
        if trait_id >= 64 { return Known::Unknown }
        let mut own = self.tables.key_domains.iter().filter(|(domain, _)| *domain == key.domain())
            .fold(0u64, |own, (_, traits)| own | traits);
        if let Some((truthy, falsy)) = self.tables.key_truthiness {
            own |= match &key {
                Key::Text(text) => if text.is_empty() { falsy } else { truthy },
                Key::Ordinal(index) => if *index == 0 { falsy } else { truthy },
                Key::Symbol(_) => truthy,
                Key::Domain(_) => 0,
            };
        }
        let bit = 1u64 << trait_id;
        if own & bit != 0 { return Known::Yes(true) }
        let undecided = self.tables.exclusive.iter().any(|&group| group & bit != 0 && group & own == 0);
        if undecided { Known::Unknown } else { Known::Yes(false) }
    }

    fn key_domain(&self, leaf: Leaf) -> Known<Option<KeyDomain>> {
        let mut answer = None;
        let mut unknown = false;
        // Only a trait one domain alone names tells domains apart.
        let (mut seen, mut shared) = (0u64, 0u64);
        for &(_, traits) in &self.tables.key_domains {
            shared |= seen & traits;
            seen |= traits;
        }
        for &(domain, traits) in &self.tables.key_domains {
            let traits = traits & !shared;
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
        Self::widened_key(keys)
    }

    /// One key for several: literals of one domain widen to that domain.
    pub(super) fn widened_key(keys: Vec<Key>) -> Known<Key> {
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

    /// The keys a member read's key expression stands for, evaluated once:
    /// constants and attribute or position keys give their one key
    /// ([`Self::key`]); any other expression each literal its value holds.
    pub(super) fn key_alternatives(&self, at: Leaf, expression: &super::Value) -> Known<Vec<Key>> {
        let single = match expression {
            super::Value::Text(_) | super::Value::Ordinal(_) => true,
            super::Value::Path(path) => path.last().is_some_and(|&last| last == super::super::POSITION
                || super::super::attribute_step(last).is_some()),
            _ => false,
        };
        if single { return self.key(at, expression).map(|key| vec![key]) }
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        self.value_keys(&self.evaluate(at, expression))
    }

    /// The distinct keys a key expression's alternatives evaluate to, in
    /// order: each literal of a finite union (`'a' | 'b'`), or a wide domain.
    pub fn value_keys(&self, value: &NodeValue) -> Known<Vec<Key>> {
        let NodeValue::Alternatives(rows) = value else { return Known::Unknown };
        let mut keys: Vec<Key> = Vec::new();
        for row in rows {
            let Known::Yes(key) = self.row_key(row) else { return Known::Unknown };
            if !keys.contains(&key) { keys.push(key); }
        }
        if keys.is_empty() { Known::Unknown } else { Known::Yes(keys) }
    }

    /// [`Self::value_keys`] with the alternatives each key comes from.
    /// An enumerating leaf (an enum type) gives one key per alternative it
    /// enumerates, as a mapped type's keys over it do.
    pub(super) fn value_key_rows(&self, value: &NodeValue) -> Known<Vec<(Key, Vec<Vec<Leaf>>)>> {
        let NodeValue::Alternatives(rows) = self.enumerated(value) else { return Known::Unknown };
        let mut keys: Vec<(Key, Vec<Vec<Leaf>>)> = Vec::new();
        for row in rows {
            let Known::Yes(key) = self.row_key(&row) else { return Known::Unknown };
            match keys.iter_mut().find(|(known, _)| known == &key) {
                Some((_, sources)) => sources.push(row),
                None => keys.push((key, vec![row])),
            }
        }
        if keys.is_empty() { Known::Unknown } else { Known::Yes(keys) }
    }

    /// The key one alternative stands for: its literal, else its domain.
    /// The leaves of an alternative hold at once, so one literal among them
    /// decides the key even where other leaves (an enum's origin, a brand)
    /// cannot say theirs.
    fn row_key(&self, row: &[Leaf]) -> Known<Key> {
        let mut literal = None;
        let mut domain = None;
        let mut unknown = false;
        for &leaf in row {
            if !self.question.budget.step() { return Known::Unknown }
            let key = match self.identity(leaf) {
                Known::Yes(Some(Identity::Value(value))) => Self::expression_key(value),
                Known::Yes(Some(Identity::Declaration(node))) => match self.key_domain(leaf) {
                    Known::Yes(Some(KeyDomain::Symbol)) => Some(Key::Symbol(node)),
                    _ => { unknown = true; None }
                },
                Known::Yes(None) => {
                    match self.key_domain(leaf) {
                        Known::Yes(Some(current)) => {
                            if domain.is_some_and(|previous| previous != current) { unknown = true }
                            domain = Some(current);
                        },
                        Known::Yes(None) => {},
                        Known::Unknown => unknown = true,
                    }
                    None
                },
                Known::Unknown => { unknown = true; None }
            };
            if let Some(key) = key {
                match &literal {
                    // A member redeclared in merged declarations (`dispose:
                    // unique symbol` in two lib files) intersects each one's
                    // own symbol; TypeScript keys the member by the first.
                    Some(Key::Symbol(_)) if matches!(key, Key::Symbol(_)) => continue,
                    Some(previous) if previous != &key => return Known::Unknown,
                    _ => literal = Some(key),
                }
            }
        }
        match (literal, domain) {
            (Some(literal), _) => Known::Yes(literal),
            (None, Some(domain)) if !unknown => Known::Yes(Key::Domain(domain)),
            _ => Known::Unknown,
        }
    }
}
