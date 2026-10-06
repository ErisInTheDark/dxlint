//! JavaScript language inputs, separate from source syntax and rule options.

use crate::expr::Value;

pub struct Environment {
    pub ecma_version: u16,
    pub source_type: Option<&'static str>,
    pub implied_strict: bool,
    pub global_return: bool,
    pub globals: Vec<Value<'static>>,
}

impl Default for Environment {
    fn default() -> Self {
        Self { ecma_version: 2026, source_type: None, implied_strict: false, global_return: false, globals: Vec::new() }
    }
}

impl Environment {
    pub fn from_values<'a>(values: impl Iterator<Item = (&'a str, &'a Value<'static>)>) -> Self {
        let mut environment = Self::default();
        for (name, value) in values {
            match (name, value) {
                ("globals", Value::Record(entries)) if !entries.is_empty() => environment.globals.push(value.clone()),
                ("ecmaVersion", Value::Num(version)) => {
                    let version = *version as u16;
                    environment.ecma_version = if (6..2015).contains(&version) { version + 2009 } else { version };
                }
                ("ecmaVersion", _) => environment.ecma_version = 2026,
                ("sourceType", Value::Str(source_type)) => environment.source_type = match &**source_type {
                    "script" => Some("script"), "module" => Some("module"), "commonjs" => Some("commonjs"), _ => None,
                },
                ("sourceType", _) => environment.source_type = None,
                ("impliedStrict", value) => environment.implied_strict = matches!(value, Value::Bool(true)),
                ("globalReturn", value) => environment.global_return = matches!(value, Value::Bool(true)),
                _ => {}
            }
        }
        environment
    }
}
