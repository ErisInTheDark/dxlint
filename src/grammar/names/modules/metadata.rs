use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use std::fmt;

#[derive(Clone, Debug)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn project(&self, projection: &str, question: &super::super::question::QueryContext) -> Result<Vec<&Self>, ()> {
        let mut values = vec![self];
        if projection.is_empty() { return Ok(Vec::new()) }
        for part in projection.split('.') {
            let (key, array) = part.strip_suffix("[]").map_or((part, false), |key| (key, true));
            if key.is_empty() || key.contains(['[', ']']) { return Err(()) }
            let mut next = Vec::new();
            for value in values {
                super::resolution::spend(question)?;
                if let Some(value) = value.get(key) {
                    if array { next.extend(value.array().ok_or(())?); } else { next.push(value); }
                }
            }
            values = next;
        }
        Ok(values)
    }

    pub fn into_value(self, question: &super::super::question::QueryContext) -> Result<crate::expr::Value<'static>, ()> {
        use crate::expr::Value;
        super::resolution::spend(question)?;
        Ok(match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(value),
            Self::Number(value) => Value::Num(value),
            Self::Text(value) => Value::Str(value.into()),
            Self::Array(values) => Value::list(values.into_iter().map(|value| value.into_value(question)).collect::<Result<Vec<_>, ()>>()?),
            Self::Object(entries) => Value::record(entries.into_iter().map(|(key, value)| Ok((key.into(), value.into_value(question)?))).collect::<Result<Vec<_>, ()>>()?),
        })
    }

    pub fn get(&self, key: &str) -> Option<&Self> {
        let Self::Object(entries) = self else { return None };
        entries.iter().find(|(name, _)| name == key).map(|(_, value)| value)
    }

    pub fn text(&self) -> Option<&str> {
        match self { Self::Text(text) => Some(text), _ => None }
    }

    pub fn array(&self) -> Option<&[Self]> {
        match self { Self::Array(values) => Some(values), _ => None }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Ordered;
        impl<'de> Visitor<'de> for Ordered {
            type Value = Json;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("JSON metadata") }
            fn visit_unit<E>(self) -> Result<Json, E> { Ok(Json::Null) }
            fn visit_bool<E>(self, value: bool) -> Result<Json, E> { Ok(Json::Bool(value)) }
            fn visit_i64<E>(self, value: i64) -> Result<Json, E> { Ok(Json::Number(value as f64)) }
            fn visit_u64<E>(self, value: u64) -> Result<Json, E> { Ok(Json::Number(value as f64)) }
            fn visit_f64<E>(self, value: f64) -> Result<Json, E> { Ok(Json::Number(value)) }
            fn visit_str<E>(self, value: &str) -> Result<Json, E> { Ok(Json::Text(value.into())) }
            fn visit_string<E>(self, value: String) -> Result<Json, E> { Ok(Json::Text(value)) }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element()? { values.push(value); }
                Ok(Json::Array(values))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
                let mut values: Vec<(String, Json)> = Vec::new();
                let mut positions = crate::fx_hash::FxHashMap::<String, usize>::default();
                while let Some((key, value)) = map.next_entry::<String, Json>()? {
                    if let Some(&position) = positions.get(&key) {
                        values[position].1 = value;
                    } else {
                        positions.insert(key.clone(), values.len());
                        values.push((key, value));
                    }
                }
                Ok(Json::Object(values))
            }
        }
        deserializer.deserialize_any(Ordered)
    }
}

pub fn parse(text: &str) -> Option<Json> {
    let mut bytes = text.as_bytes().to_vec();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => at = string_end(&bytes, at)?,
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                while at < bytes.len() && !matches!(bytes[at], b'\r' | b'\n') {
                    bytes[at] = b' ';
                    at += 1;
                }
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                bytes[at] = b' ';
                bytes[at + 1] = b' ';
                at += 2;
                while at < bytes.len() && !(bytes[at] == b'*' && bytes.get(at + 1) == Some(&b'/')) {
                    if !matches!(bytes[at], b'\r' | b'\n') { bytes[at] = b' '; }
                    at += 1;
                }
                if at + 1 >= bytes.len() { return None }
                bytes[at] = b' ';
                bytes[at + 1] = b' ';
                at += 2;
            }
            _ => at += 1,
        }
    }
    at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => at = string_end(&bytes, at)?,
            b',' => {
                let next = bytes[at + 1..].iter().find(|byte| !byte.is_ascii_whitespace());
                if matches!(next, Some(b'}' | b']')) { bytes[at] = b' '; }
                at += 1;
            }
            _ => at += 1,
        }
    }
    serde_json::from_slice(&bytes).ok()
}

fn string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut at = start + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'"' => return Some(at + 1),
            _ => at += 1,
        }
    }
    None
}
