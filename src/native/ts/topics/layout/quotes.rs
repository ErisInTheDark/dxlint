//! Property quote requirements and their grouping owners.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::{Lexer, Tok};
use crate::native::ts::statics;
use std::sync::LazyLock;

use super::lex;

pub(super) fn property_quote_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    if matches!(syntax.kind(node), Some(K::Import | K::Export)) {
        let Some(from) = syntax.get(node, F::From) else { return Value::list(Vec::new()) };
        let lexed = file.cached(lex);
        let at = lexed.0.partition_point(|token| token.start < syntax.tree.node(from).end);
        let end = lexed.0.partition_point(|token| token.start < syntax.tree.node(node).end);
        let tokens = &lexed.0[at..end];
        if tokens.first().is_none_or(|token| token.tok != Tok::Ident || !matches!(&syntax.src[token.start as usize..token.end as usize], "with" | "assert")) {
            return Value::list(Vec::new());
        }
        let mut sites = Vec::new();
        let mut at = 2;
        while let Some(key) = tokens.get(at).filter(|token| matches!(token.tok, Tok::Ident | Tok::Str)) {
            if tokens.get(at + 1).is_none_or(|token| token.tok != Tok::P(":")) {
                break;
            }
            let raw = &syntax.src[key.start as usize..key.end as usize];
            let quoted = key.tok == Tok::Str;
            let name = if quoted { crate::native::ts::regex::cook(raw).unwrap_or_else(|| raw.to_owned()) } else { raw.to_owned() };
            for group in [false, true] {
                sites.push(quote_site(name.clone(), quoted, false, key.start as usize, group));
            }
            at += 3;
            if tokens.get(at).is_none_or(|token| token.tok != Tok::P(",")) {
                break;
            }
            at += 1;
        }
        return Value::list(sites);
    }
    let (group, nodes) = match syntax.kind(node) {
        Some(K::Object) => (true, syntax.all(node, F::Property).collect()),
        Some(K::TypeLit | K::Interface | K::Enum) => (true, syntax.all(node, F::Member).collect()),
        Some(K::Prop | K::PatternProp | K::PropSig | K::EnumMember) => (false, vec![node]),
        _ => return Value::list(Vec::new()),
    };
    Value::list(nodes.into_iter().filter_map(|id| {
        if !matches!(syntax.kind(id), Some(K::Prop | K::PatternProp | K::PropSig | K::MethodSig | K::EnumMember)) {
            return None;
        }
        if matches!(syntax.kind(id), Some(K::Prop | K::PatternProp)) && syntax.get(id, F::Value).is_none() {
            return None;
        }
        let key = syntax.get(id, if syntax.is(id, K::EnumMember) { F::Name } else { F::Key })?;
        if syntax.is(key, K::Computed) {
            return None;
        }
        let quoted = syntax.is(key, K::String);
        let numeric = matches!(syntax.kind(key), Some(K::Number | K::BigInt));
        let name = if syntax.is(key, K::Ident) { syntax.text(key).to_owned() } else { statics::literal_text(syntax, key)? };
        Some(quote_site(name, quoted, numeric, syntax.tree.node(id).start as usize, group))
    }).collect())
}

fn quote_site(name: String, quoted: bool, numeric: bool, offset: usize, group: bool) -> Value<'static> {
    static IDENTIFIER: LazyLock<regex_automata::meta::Regex> = LazyLock::new(|| regex_automata::meta::Regex::new(r"^[$_\p{ID_Start}][$\u{200c}\u{200d}\p{ID_Continue}]*$").expect("identifier pattern"));
    let mut lexer = Lexer::new(&name);
    let token = lexer.next_token();
    let whole = token.start == 0 && token.end as usize == name.len() && lexer.next_token().tok == Tok::Eof;
    let redundant_number = whole && token.tok == Tok::Num && statics::number(&name).is_some_and(|n| crate::expr::value::js_number_text(n) == name);
    let identifier = if name.contains('\\') { crate::native::ts::regex::cook(&format!("\"{name}\"")).unwrap_or_default() } else { name.clone() };
    let redundant = whole && token.tok == Tok::Ident && IDENTIFIER.is_match(&identifier) || redundant_number;
    let keyword = matches!(name.as_str(),
        "abstract" | "boolean" | "break" | "byte" | "case" | "catch" | "char" | "class" | "const" | "continue" | "debugger" | "default" | "delete" | "do" | "double" | "else" | "enum" | "export" | "extends" | "false" | "final" | "finally" | "float" | "for" | "function" | "goto" | "if" | "implements" | "import" | "in" | "instanceof" | "int" | "interface" | "long" | "native" | "new" | "null" | "package" | "private" | "protected" | "public" | "return" | "short" | "static" | "super" | "switch" | "synchronized" | "this" | "throw" | "throws" | "transient" | "true" | "try" | "typeof" | "var" | "void" | "volatile" | "while" | "with"
    );
    Value::record(vec![
        ("group".into(), Value::Bool(group)),
        ("quoted".into(), Value::Bool(quoted)),
        ("numeric".into(), Value::Bool(numeric)),
        ("redundant".into(), Value::Bool(redundant)),
        ("redundant_number".into(), Value::Bool(redundant_number)),
        ("keyword".into(), Value::Bool(keyword)),
        ("name".into(), Value::from(name)),
        ("offset".into(), Value::from(offset)),
    ])
}
