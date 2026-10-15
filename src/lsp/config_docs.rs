//! Language features inside `.zlint` files, read from the config's symbol
//! index: hover, go to definition, references, completion and outline.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::convert;
use super::uri;
use crate::config::ast::Severity;
use crate::config::source::{FileId, SourceFile, Span};
use crate::config::std_lib;
use crate::engine::symbols::{Kind, Symbol, Symbols};

/// One `.zlint` file of a built config.
pub struct ConfigFile<'a> {
    pub symbols: &'a Symbols,
    pub file: FileId,
}

impl ConfigFile<'_> {
    fn text(&self) -> &str {
        &self.symbols.sources.get(self.file).text
    }

    /// The byte offset of an LSP position in this file (as it was built).
    fn offset(&self, params: &Value) -> u32 {
        let f = self.symbols.sources.get(self.file);
        let pos = &params["position"];
        let line = pos["line"].as_u64().unwrap_or(0) as u32;
        let character = pos["character"].as_u64().unwrap_or(0) as u32;
        f.lines.from_utf16(&f.text, line, character) as u32
    }

    /// The word around `offset` (`[A-Za-z0-9_]`, plus `-`/`:` inside rule
    /// names).
    fn word(&self, offset: u32) -> &str {
        let text = self.text();
        let b = text.as_bytes();
        let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b':';
        let mut start = offset as usize;
        while start > 0 && is_word(b[start - 1]) {
            start -= 1;
        }
        let mut end = offset as usize;
        while end < b.len() && is_word(b[end]) {
            end += 1;
        }
        &text[start..end]
    }

    fn at(&self, offset: u32) -> Option<usize> {
        self.symbols.at(self.file, offset).or_else(|| {
            let word = self.word(offset);
            self.symbols.get(Kind::Keyword, word).or_else(|| self.symbols.named(word).first().copied())
        })
    }

    pub fn hover(&self, params: &Value) -> Value {
        let Some(id) = self.at(self.offset(params)) else { return Value::Null };
        json!({ "contents": { "kind": "markdown", "value": self.symbols.describe(id) } })
    }

    pub fn definition(&self, params: &Value) -> Value {
        let Some(id) = self.at(self.offset(params)) else { return Value::Null };
        Value::Array(self.symbols.symbols[id].defs.iter().filter_map(|s| location(self.symbols, *s)).collect())
    }

    pub fn references(&self, params: &Value) -> Value {
        let Some(id) = self.at(self.offset(params)) else { return Value::Null };
        let declarations = params["context"]["includeDeclaration"].as_bool().unwrap_or(true);
        let defs = self.symbols.symbols[id].defs.iter().copied().filter(|_| declarations);
        Value::Array(self.symbols.uses_of(id).chain(defs).filter_map(|s| location(self.symbols, s)).collect())
    }

    /// The outline: what this file defines.
    pub fn outline(&self) -> Value {
        let f = self.symbols.sources.get(self.file);
        let mut out = Vec::new();
        for sym in &self.symbols.symbols {
            let Some(kind) = symbol_kind(sym.kind) else { continue };
            for d in sym.defs.iter().filter(|d| d.file == self.file) {
                let range = convert::range(&f.text, &f.lines, d.start as usize, d.end as usize);
                out.push(json!({
                    "name": sym.name,
                    "detail": sym.summary.chars().take(80).collect::<String>(),
                    "kind": kind,
                    "range": range,
                    "selectionRange": range,
                }));
            }
        }
        Value::Array(out)
    }

    /// Completions for what the line up to the cursor asks for.
    pub fn complete(&self, params: &Value) -> Value {
        let offset = self.offset(params) as usize;
        let text = self.text();
        let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
        let before = &text[line_start..offset];
        let trimmed = before.trim_start();
        let items: Vec<Value> = if let Some(rest) = trimmed.strip_prefix("import ") {
            if rest.contains(char::is_whitespace) {
                Vec::new()
            } else {
                // Built-in languages and rulesets; anything else is a path.
                let langs = std_lib::names().filter(|n| !n.contains('/') && *n != "core" && *n != "whitespace");
                langs
                    .map(|n| json!({ "label": format!(".{}", std_lib::short_name(n)), "kind": 9, "detail": format!("built-in language {n}, its rules defined and off") }))
                    .chain(std_lib::rulesets().map(|n| json!({ "label": format!("rules:{n}"), "kind": 9, "detail": "built-in ruleset" })))
                    .collect()
            }
        } else if trimmed.starts_with("rule ") && !trimmed[5..].contains(char::is_whitespace) {
            // `rule ` without `in`: turns a defined rule on.
            self.of_kind(Kind::Rule).map(|(i, s)| item(i, s)).collect()
        } else if let Some(rest) = trimmed.strip_prefix("rule ").filter(|r| !r.contains('{') && before.ends_with(char::is_whitespace)) {
            // After `rule name `: a severity or `set`; after `set `, the
            // values of the rule's default option.
            let name = rest.split_whitespace().next().unwrap_or("");
            if rest.split_whitespace().last() == Some("set") {
                match self.symbols.get(Kind::Option, name) {
                    Some(id) => self.symbols.symbols[id].fields.iter().map(|v| json!({ "label": v, "kind": 12, "detail": format!("default option of {name}") })).collect(),
                    None => Vec::new(),
                }
            } else {
                let mut items: Vec<Value> = Severity::NAMES.iter().map(|s| json!({ "label": s, "kind": 14, "detail": "severity" })).collect();
                if self.symbols.get(Kind::Option, name).is_some() {
                    items.push(json!({ "label": "set", "kind": 14, "detail": "set the default option" }));
                }
                items
            }
        } else if let Some(rest) = trimmed.strip_prefix("set ") {
            // `set ` inside a rule or language block: that owner's options; after
            // `set option `, the literal values its type takes.
            match (enclosing_options(&text[..line_start]), rest.split_once(char::is_whitespace)) {
                (Some(rule), None) => self.of_kind(Kind::Option).filter_map(|(i, s)| s.name.strip_prefix(&format!("{rule}.")).map(|o| item_named(i, s, o))).collect(),
                (Some(rule), Some((option, value))) if !value.contains(char::is_whitespace) => match self.symbols.get(Kind::Option, &format!("{rule}.{option}")) {
                    Some(id) => self.symbols.symbols[id].fields.iter().map(|v| json!({ "label": v, "kind": 12, "detail": format!("value of {option}") })).collect(),
                    None => Vec::new(),
                },
                _ => Vec::new(),
            }
        } else if let Some(layer) = dotted(before) {
            match self.symbols.get(Kind::Layer, layer) {
                Some(id) => self.symbols.symbols[id]
                    .fields
                    .iter()
                    .map(|f| json!({ "label": f, "kind": 5, "detail": format!("field of {layer}") }))
                    .chain(self.of_kind(Kind::Attr)
                        .filter(|(_, attr)| !self.symbols.symbols[id].fields.contains(&attr.name))
                        .map(|(id, attr)| item(id, attr)))
                    .collect(),
                // `field.`, `parent.`, `children.`...: what a node has.
                None => [Kind::Attr, Kind::Fact, Kind::Builtin].iter().flat_map(|&k| self.of_kind(k)).map(|(i, s)| item(i, s)).collect(),
            }
        } else if is_layer_list(trimmed) {
            [Kind::Layer, Kind::Group, Kind::Lang].iter().flat_map(|&k| self.of_kind(k)).map(|(i, s)| item(i, s)).collect()
        } else {
            [Kind::Attr, Kind::Builtin, Kind::Fact, Kind::Function, Kind::Relative, Kind::Macro, Kind::Layer, Kind::Keyword]
                .iter()
                .flat_map(|&k| self.of_kind(k))
                // Macros only expand as `@name`.
                .map(|(i, s)| if s.kind == Kind::Macro { item_named(i, s, &format!("@{}", s.name)) } else { item(i, s) })
                .collect()
        };
        json!({ "isIncomplete": false, "items": items })
    }

    fn of_kind(&self, kind: Kind) -> impl Iterator<Item = (usize, &Symbol)> + '_ {
        self.symbols.symbols.iter().enumerate().filter(move |(_, s)| s.kind == kind)
    }
}

/// `in a, b`, `group g a`, `drop group g`, `is (` before the cursor, with no
/// `layer`/`match`/`rule`/`let` yet: a list of layers.
fn is_layer_list(before: &str) -> bool {
    let words: Vec<&str> = before.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
    let head = words.first().copied().unwrap_or("");
    // A rule's `indent [in] a, b.field [by n]` line.
    if head == "indent" {
        return !words.contains(&"by");
    }
    let past_head = words.iter().any(|w| matches!(*w, "layer" | "match" | "rule" | "let" | "indent" | "forward"));
    (matches!(head, "in" | "group") || before.ends_with("is ") || before.ends_with("is (") || before.ends_with("not ") || before.ends_with("not (") || before.ends_with(", ")) && !past_head
}

/// The name before a `.` the cursor follows: `ts_call.` -> `ts_call`.
fn dotted(before: &str) -> Option<&str> {
    let head = before.strip_suffix('.').or_else(|| {
        let i = before.rfind('.')?;
        before[i + 1..].chars().all(|c| c.is_ascii_alphanumeric() || c == '_').then_some(&before[..i])
    })?;
    let start = head.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
    let name = &head[start..];
    (!name.is_empty()).then_some(name)
}

/// The owner of a rule or language configuration block still open at `text`.
fn enclosing_options(text: &str) -> Option<&str> {
    let mut depth = 0i32;
    for line in text.lines().rev() {
        depth += line.matches('}').count() as i32 - line.matches('{').count() as i32;
        if depth < 0 {
            let mut words = line.split_whitespace();
            return matches!(words.next(), Some("rule" | "language")).then(|| words.next()).flatten();
        }
    }
    None
}

fn item(_id: usize, s: &Symbol) -> Value {
    item_named(_id, s, &s.name)
}

fn item_named(_id: usize, s: &Symbol, label: &str) -> Value {
    json!({
        "label": label,
        "kind": completion_kind(s.kind),
        "detail": s.summary.chars().take(100).collect::<String>(),
        "documentation": { "kind": "markdown", "value": s.doc },
    })
}

fn completion_kind(k: Kind) -> u8 {
    match k {
        Kind::Rule => 23,
        Kind::Layer => 7,
        Kind::Group => 13,
        Kind::Attr => 10,
        Kind::Macro => 15,
        Kind::Lang => 9,
        Kind::Option => 5,
        Kind::Local => 6,
        Kind::Field => 5,
        Kind::Builtin => 6,
        Kind::Fact => 10,
        Kind::Function => 3,
        Kind::Relative => 6,
        Kind::Keyword => 14,
        Kind::File => 17,
        Kind::Schema => 22,
    }
}

fn symbol_kind(k: Kind) -> Option<u8> {
    Some(match k {
        Kind::Rule => 24,
        Kind::Layer => 5,
        Kind::Group => 10,
        Kind::Attr => 7,
        Kind::Macro => 14,
        Kind::Lang => 2,
        Kind::Option => 8,
        Kind::Local => 13,
        _ => return None,
    })
}

/// An LSP location for a span; std modules are opened from read-only
/// copies.
pub fn location(symbols: &Symbols, span: Span) -> Option<Value> {
    let f = symbols.sources.get(span.file);
    let path = source_path(f)?;
    let range = convert::range(&f.text, &f.lines, span.start as usize, span.end as usize);
    Some(json!({ "uri": uri::from_path(&path), "range": range }))
}

/// A config file an editor can open: its own path, or a std copy.
pub fn source_path(f: &SourceFile) -> Option<PathBuf> {
    match &f.path {
        Some(p) => Some(p.clone()),
        None => std_copy(&f.name, &f.text),
    }
}

/// Where std modules are written so editors can open them.
fn std_dir() -> PathBuf {
    std::env::temp_dir().join(format!("zlint-std-{}", env!("CARGO_PKG_VERSION")))
}

/// A read-only file holding a built-in module's text (`std/ts/problems`,
/// `rules/format`).
fn std_copy(name: &str, text: &str) -> Option<PathBuf> {
    if !(name.starts_with("std/") || name.starts_with("rules/")) {
        return None;
    }
    let path = std_dir().join(format!("{name}.zlint"));
    if std::fs::read_to_string(&path).ok().as_deref() != Some(text) {
        std::fs::create_dir_all(path.parent()?).ok()?;
        // A stale read-only copy has to be writable again first.
        if let Ok(meta) = std::fs::metadata(&path) {
            let mut perms = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(&path, perms);
        }
        std::fs::write(&path, text).ok()?;
        if let Ok(meta) = std::fs::metadata(&path) {
            let mut perms = meta.permissions();
            perms.set_readonly(true);
            let _ = std::fs::set_permissions(&path, perms);
        }
    }
    Some(path)
}

/// The built-in module's name (`std/ts/problems`) when `path` is one of the
/// read-only copies.
pub fn std_module(path: &Path) -> Option<String> {
    let rel = path.strip_prefix(std_dir()).ok()?.with_extension("");
    Some(rel.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load_text, source::Disk};
    use crate::engine::build;

    #[test]
    fn linked_nodes_complete_and_hover_as_registered_queries() {
        let text = "import .ts\nin ts_call.callee rule field info text\nin ts_identifier rule query info declaration.offset\nin ts_program.binding_declarations rule names info text\nin ts_call let callee node\n";
        let built = build(load_text("links.zlint", text, &Disk), PathBuf::from("/root"));
        assert!(built.errors.is_empty(), "{:?}", built.errors);
        let symbols = built.symbols.expect("symbols");
        let (file, _) = symbols.sources.files().find(|(_, source)| source.text.as_ref() == text).expect("source");
        let config = ConfigFile { symbols: &symbols, file };
        let completions = config.complete(&json!({ "position": { "line": 1, "character": "in ts_call.".len() } }));
        let items = completions["items"].as_array().expect("items");
        assert!(items.iter().any(|item| item["label"] == "callee"), "ordinary fields remain available");
        assert!(items.iter().any(|item| item["label"] == "references"), "computed attributes available alongside fields");
        assert_eq!(items.iter().filter(|item| item["label"] == "callee").count(), 1,
            "a field and attribute sharing a name must not duplicate completion");
        let hover = config.hover(&json!({ "position": { "line": 2, "character": "in ts_identifier rule query info decl".len() } }));
        assert!(!hover.is_null(), "computed node attribute has declaration documentation");
        let hover = config.hover(&json!({ "position": { "line": 3, "character": "in ts_program.binding_".len() } }));
        assert!(!hover.is_null(), "dotted target refers to attribute documentation");
    }

    #[test]
    fn language_settings_complete_and_link_to_their_declarations() {
        let text = "import .ts\nlanguage .js {\n  set sourceType \"commonjs\"\n  set globals { app: \"readonly\" }\n}\n";
        let built = build(load_text("environment.zlint", text, &Disk), PathBuf::from("/root"));
        assert!(built.errors.is_empty(), "{:?}", built.errors);
        let symbols = built.symbols.expect("symbols");
        let (file, _) = symbols.sources.files().find(|(_, source)| source.text.as_ref() == text).expect("source");
        let config = ConfigFile { symbols: &symbols, file };
        let names = config.complete(&json!({ "position": { "line": 2, "character": 6 } }));
        assert!(names["items"].as_array().expect("items").iter().any(|item| item["label"] == "sourceType"));
        let values = config.complete(&json!({ "position": { "line": 2, "character": "  set sourceType ".len() } }));
        assert!(!values["items"].as_array().expect("values").is_empty());
        let definition = config.definition(&json!({ "position": { "line": 2, "character": 8 } }));
        assert!(!definition.as_array().expect("definitions").is_empty());
    }
}
