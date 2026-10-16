//! One TypeScript/JavaScript file as the engine sees it: its syntax and the
//! token streams rules scan. No semantics: those live in `std/ts`.

use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use super::comments::{comments, Comment};
use super::syntax::Syntax;
use super::tokens::{lex, lex_comments, Lexed};
use crate::engine::frontend::{self, Native};
use crate::expr::Value;
use crate::fx_hash::FxHashMap;

pub struct TsFile<'t> {
    pub syntax: Syntax<'t>,
    comments: OnceCell<Vec<Comment>>,
    cache: RefCell<FxHashMap<TypeId, Rc<dyn Any>>>,
}

impl<'t> TsFile<'t> {
    pub fn new(syntax: Syntax<'t>) -> Self {
        Self { syntax, comments: OnceCell::new(), cache: RefCell::default() }
    }

    /// Reuse the complete stream already scanned for token attachment.
    pub(crate) fn with_tokens(mut self, tokens: Option<Lexed>) -> Self {
        if let Some(tokens) = tokens {
            self.cache.get_mut().insert(TypeId::of::<Lexed>(), Rc::new(tokens));
        }
        self
    }

    /// A whole-file result of type `T`, computed on first use (one per type).
    pub fn cached<T: 'static>(&self, init: impl FnOnce(&Self) -> T) -> Rc<T> {
        let key = TypeId::of::<T>();
        if let Some(value) = self.cache.borrow().get(&key) {
            return value.clone().downcast::<T>().expect("keyed by type");
        }
        let value: Rc<T> = Rc::new(init(self));
        self.cache.borrow_mut().insert(key, value.clone());
        value
    }

    pub fn comments(&self) -> &[Comment] {
        self.comments.get_or_init(|| comments(&self.syntax))
    }

    /// Parser-aware tokens inside a node; trivia outside literals is dropped.
    pub fn tokens(&self, node: u32) -> Vec<&str> {
        let span = self.syntax.tree.node(node);
        let code = self.cached(lex);
        let from = code.0.partition_point(|token| token.start < span.start);
        code.0[from..].iter().take_while(|token| token.end <= span.end)
            .map(|token| &self.syntax.src[token.start as usize..token.end as usize])
            .collect()
    }
}

/// What an identifier's text means: its escapes decoded (`a` is `a`).
pub fn decoded_name(text: &str) -> Cow<'_, str> {
    if !text.contains('\\') { return Cow::Borrowed(text) }
    Cow::Owned(crate::expr::js::strings::decode_literal(&format!("\"{text}\"")).unwrap_or_else(|| text.to_string()))
}

impl Native for TsFile<'_> {
    fn frontend(&self) -> &'static str {
        "ts"
    }

    fn fact(&self, _fact: usize, _node: u32) -> Value<'static> {
        Value::Null
    }

    fn inside_token(&self, at: usize) -> bool {
        use super::lexer::Tok;
        let tokens = self.cached(lex_comments);
        let after = tokens.0.partition_point(|token| (token.start as usize) < at);
        // JSX text is content laid out by its own lines, not one token.
        after.checked_sub(1).map(|i| &tokens.0[i]).is_some_and(|token| token.end as usize > at && matches!(token.tok, Tok::Template | Tok::Str | Tok::P("/*")))
    }

    fn tokens(&self) -> Option<Rc<frontend::Tokens>> {
        Some(self.cached(|file| frontend::Tokens(file.cached(lex).0.iter().map(|token| (token.start, token.end)).collect())))
    }

    fn name_key<'s>(&self, text: &'s str) -> Cow<'s, str> {
        decoded_name(text)
    }
}
