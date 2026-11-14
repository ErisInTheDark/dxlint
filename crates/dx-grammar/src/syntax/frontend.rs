//! A declared parser as a language's front end (`native <name>` naming a
//! `parser <name> in <lang>`): its node kinds and fields become layers, its
//! visible token kinds become token layers, and the spans it read answer
//! `token(...)` and the questions about what lies inside a token.

use std::any::Any;
use std::borrow::Cow;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use super::{CompiledParser, Context, Registration, TokenSpan, TriviaSpan};
use dx_config::ast::ParserDecl;
use crate::frontend::{FieldShape, FileInput, Frontend, Native, Parsed, Tokens};
use dx_ir::value::Value;
use crate::{Edge, Node, TEXT};

pub struct DeclaredFrontend {
    parser: Arc<CompiledParser>,
    registration: Registration,
    /// Node kinds, then the visible token kinds.
    kinds: Vec<String>,
    nodes: usize,
    /// Every node's field names, then `token`.
    fields: Vec<String>,
    kind_fields: Vec<Vec<u16>>,
    literal: Vec<bool>,
    root: u16,
    /// Each declared token's visible kind.
    token_kinds: Vec<Option<u16>>,
    /// Declared tokens laid out by their own lines (`token x /.../ lines`).
    token_lines: Vec<bool>,
    /// Flags that read same-name boolean language options.
    option_flags: Vec<(String, u32)>,
    /// Flags on in files their globs match ([`ParserDecl::file_flags`]).
    file_flags: Vec<(dx_core::path_globs::PathGlobs, u32)>,
    /// `name_escape` patterns ([`ParserDecl::name_escapes`]).
    name_escapes: Vec<Arc<regex_automata::meta::Regex>>,
}

/// What one parse leaves for token nodes and the file's facts.
struct Spans {
    tokens: Box<[TokenSpan]>,
    /// Only the gaps [`opens_token`] holds: no other gap can answer
    /// [`Native::inside_token`], and gaps never overlap.
    trivia: Box<[TriviaSpan]>,
}

/// Whether trivia at `start` (a comment, not whitespace) counts as a token.
fn opens_token(text: &str, start: u32) -> bool {
    text[start as usize..].starts_with(|c: char| !c.is_whitespace())
}

impl DeclaredFrontend {
    pub fn build(declaration: &ParserDecl) -> Result<Self, String> {
        let parser = CompiledParser::shared(&declaration.definition)?;
        let nodes = parser.nodes();
        let mut kinds: Vec<String> = nodes.iter().map(|node| node.name.clone()).collect();
        let mut fields: Vec<String> = Vec::new();
        let mut labels = Vec::with_capacity(nodes.len());
        for node in nodes {
            labels.push(node.fields.iter().map(|field| field_id(&field.name, &mut fields)).collect::<Result<Vec<_>, _>>()?);
        }
        let kind_fields = labels.clone();
        field_id("token", &mut fields)?;
        let mut token_kinds = Vec::with_capacity(parser.tokens().len());
        for token in parser.tokens() {
            token_kinds.push(match &token.visible_kind {
                Some(kind) => {
                    let index = kinds.iter().position(|known| known == kind).unwrap_or_else(|| {
                        kinds.push(kind.clone());
                        kinds.len() - 1
                    });
                    Some(u16::try_from(index).map_err(|_| "declared parser kind count exceeds node ids")?)
                }
                None => None,
            });
        }
        let token_lines = parser.tokens().iter().map(|token| token.lines).collect();
        let literal = nodes.iter().map(|node| node.literal_text).collect();
        // `root ts_program = ...`: the root production is named for the node it builds.
        let root = nodes.iter().position(|node| node.name == declaration.definition.root)
            .ok_or_else(|| format!("its root `{}` names no node", declaration.definition.root))?;
        let matchers = (0..nodes.len()).map(u32::try_from).collect::<Result<Vec<_>, _>>()
            .map_err(|_| "declared parser kind count exceeds node ids")?;
        let option_flags = declaration.option_flags.iter()
            .filter_map(|flag| parser.flags().iter().position(|known| known == flag).map(|bit| (flag.clone(), bit as u32)))
            .collect();
        let mut file_flags = Vec::new();
        for (flag, globs) in &declaration.file_flags {
            let globs = dx_core::path_globs::PathGlobs::new(globs.iter().map(String::as_str))
                .map_err(|(glob, message)| format!("`{glob}`: {message}"))?;
            if let Some(bit) = parser.flags().iter().position(|known| known == flag) { file_flags.push((globs, bit as u32)); }
        }
        let node_count = nodes.len();
        Ok(Self {
            registration: Registration { matchers, labels, text_matcher: TEXT },
            parser,
            kinds,
            nodes: node_count,
            fields,
            kind_fields,
            literal,
            root: root as u16,
            token_kinds,
            token_lines,
            option_flags,
            file_flags,
            name_escapes: declaration.name_escapes.iter().map(|escape| dx_ir::re::compile(escape)).collect::<Result<_, _>>()?,
        })
    }

    fn token_field_id(&self) -> u16 {
        (self.fields.len() - 1) as u16
    }
}

/// `name`'s number among `fields`, added when new: one table for every node kind.
fn field_id(name: &str, fields: &mut Vec<String>) -> Result<u16, String> {
    let index = fields.iter().position(|field| field == name).unwrap_or_else(|| {
        fields.push(name.to_owned());
        fields.len() - 1
    });
    u16::try_from(index).map_err(|_| "declared parser field count exceeds edge labels".to_owned())
}

impl Frontend for DeclaredFrontend {
    fn kinds(&self) -> &[String] {
        &self.kinds
    }

    fn fields(&self) -> &[String] {
        &self.fields
    }

    fn kind_fields(&self, kind: u16) -> Vec<u16> {
        self.kind_fields.get(usize::from(kind)).cloned().unwrap_or_default()
    }

    fn token_field(&self) -> u16 {
        self.token_field_id()
    }

    fn is_token(&self, kind: u16) -> bool {
        usize::from(kind) >= self.nodes || self.token_kinds.contains(&Some(kind))
    }

    fn is_literal_text(&self, kind: u16) -> bool {
        self.literal.get(usize::from(kind)).copied().unwrap_or(false)
    }

    fn root_kind(&self) -> u16 {
        self.root
    }

    fn field_shape(&self, kind: u16, field: u16) -> FieldShape {
        if field == self.token_field_id() {
            let mut kinds: Vec<u16> = self.token_kinds.iter().flatten().copied().collect();
            kinds.sort_unstable();
            kinds.dedup();
            return FieldShape { kinds, text: false, any: false };
        }
        let Some(node) = self.parser.nodes().get(usize::from(kind)) else { return FieldShape::default() };
        let Some(name) = self.fields.get(usize::from(field)) else { return FieldShape::default() };
        let Some(field) = node.fields.iter().find(|candidate| &candidate.name == name) else { return FieldShape::default() };
        let mut shape = FieldShape { kinds: Vec::new(), text: field.text, any: false };
        for kind in &field.kinds {
            match self.kinds.iter().position(|known| known == kind) {
                Some(index) => shape.kinds.push(index as u16),
                None => shape.any = true,
            }
        }
        shape
    }

    fn parse(&self, text: &str, path: &Path, options: &[(&str, &Value<'static>)]) -> Result<Parsed, (u32, String)> {
        let mut context = Context::default();
        for (flag, bit) in &self.option_flags {
            // Defaults come first, settings after: the last one holds.
            if let Some((_, Value::Bool(true))) = options.iter().rfind(|(name, _)| name == flag) {
                context.flags |= 1 << bit;
            }
        }
        for (globs, bit) in &self.file_flags {
            if globs.matches(path) { context.flags |= 1 << bit; }
        }
        let parsed = self.parser.parse_root(text, context, &self.registration)
            .map_err(|failure| (failure.offset, format!("expected {}", failure.expected.join(" or "))))?;
        let trivia = parsed.trivia.into_iter().filter(|trivia| opens_token(text, trivia.start)).collect();
        let state: Arc<dyn Any + Send + Sync> = Arc::new(Spans { tokens: parsed.tokens.into_boxed_slice(), trivia });
        Ok(Parsed { nodes: parsed.nodes, root: parsed.root, state: Some(state) })
    }

    fn attach_tokens(&self, _text: &str, parsed: &mut Parsed, wanted: &[bool]) {
        let Some(spans) = parsed.state.as_ref().and_then(|state| state.downcast_ref::<Spans>()) else { return };
        let visible = |token: &TokenSpan| self.token_kinds.get(usize::from(token.token)).copied().flatten();
        if !spans.tokens.iter().any(|token| visible(token).is_some_and(|kind| wanted.get(usize::from(kind)).copied().unwrap_or(false))) { return }
        let tokens = &spans.tokens;
        let nodes = &parsed.nodes;
        let mut owned = Vec::new();
        let mut stack = vec![parsed.root];
        let mut children = Vec::new();
        while let Some(id) = stack.pop() {
            let node = &nodes[id as usize];
            stack.extend(node.children.iter().filter(|edge| nodes[edge.node as usize].matcher != TEXT).map(|edge| edge.node));
            let mut at = tokens.partition_point(|token| token.start < node.start);
            // A node written as one token already owns it.
            if tokens.get(at).is_some_and(|token| (token.start, token.end) == (node.start, node.end)) { continue }
            children.clear();
            children.extend(node.children.iter().map(|edge| &nodes[edge.node as usize]).filter(|child| child.matcher != TEXT));
            if !children.windows(2).all(|pair| pair[0].start <= pair[1].start) {
                children.sort_by_key(|child| child.start);
            }
            let mut next_child = children.iter().peekable();
            while let Some(token) = tokens.get(at).filter(|token| token.end <= node.end) {
                while next_child.next_if(|child| child.end <= token.start).is_some() {}
                match next_child.peek() {
                    Some(child) if child.start <= token.start && token.end <= child.end => {
                        at = tokens.partition_point(|token| token.start < child.end);
                        next_child.next();
                    }
                    _ => {
                        if let Some(kind) = visible(token).filter(|&kind| wanted.get(usize::from(kind)).copied().unwrap_or(false)) {
                            owned.push((id, kind, token.start, token.end));
                        }
                        at += 1;
                    }
                }
            }
        }
        let label = self.token_field_id();
        for (owner, kind, start, end) in owned {
            let id = parsed.nodes.len() as u32;
            parsed.nodes.push(Node { matcher: u32::from(kind), start, end, children: Vec::new() });
            parsed.nodes[owner as usize].children.push(Edge { label, node: id });
        }
    }

    fn file<'t>(&'t self, input: FileInput<'t, '_>) -> Box<dyn Native + 't> {
        let spans = input.state.clone().and_then(|state| state.downcast::<Spans>().ok());
        Box::new(DeclaredFile { spans, name_escapes: &self.name_escapes, token_lines: &self.token_lines, tokens: Default::default() })
    }
}

struct DeclaredFile<'t> {
    spans: Option<Arc<Spans>>,
    token_lines: &'t [bool],
    name_escapes: &'t [Arc<regex_automata::meta::Regex>],
    /// The code tokens as `token(...)` reads them, built on first use.
    tokens: std::cell::OnceCell<Option<Rc<Tokens>>>,
}

/// `text` with every `name_escape` match replaced by the character its
/// first matching group names in hex; unchanged when a code point is invalid.
fn decode_name<'s>(text: &'s str, escapes: &[Arc<regex_automata::meta::Regex>]) -> Cow<'s, str> {
    let mut decoded = Cow::Borrowed(text);
    for regex in escapes {
        if !regex.is_match(decoded.as_ref()) { continue }
        let mut out = String::with_capacity(decoded.len());
        let mut last = 0;
        for captures in regex.captures_iter(decoded.as_ref()) {
            let Some(found) = captures.get_match() else { continue };
            let code = (1..captures.group_len()).find_map(|group| captures.get_group(group))
                .and_then(|span| u32::from_str_radix(&decoded[span.range()], 16).ok())
                .and_then(char::from_u32);
            let Some(code) = code else { return Cow::Borrowed(text) };
            out.push_str(&decoded[last..found.start()]);
            out.push(code);
            last = found.end();
        }
        out.push_str(&decoded[last..]);
        decoded = Cow::Owned(out);
    }
    decoded
}

impl Native for DeclaredFile<'_> {
    fn inside_token(&self, at: usize) -> bool {
        let Some(spans) = &self.spans else { return false };
        let at = at as u32;
        let token = spans.tokens.partition_point(|token| token.start < at).checked_sub(1)
            .map(|index| &spans.tokens[index])
            .is_some_and(|token| token.end > at && !self.token_lines.get(usize::from(token.token)).copied().unwrap_or(false));
        token || spans.trivia.partition_point(|trivia| trivia.start < at).checked_sub(1)
            .map(|index| &spans.trivia[index])
            .is_some_and(|trivia| trivia.end > at)
    }

    fn tokens(&self) -> Option<Rc<Tokens>> {
        self.tokens.get_or_init(|| {
            let spans = self.spans.as_ref()?;
            Some(Rc::new(Tokens(spans.tokens.iter().map(|token| (token.start, token.end)).collect())))
        }).clone()
    }

    fn name_key<'s>(&self, text: &'s str) -> Cow<'s, str> {
        decode_name(text, self.name_escapes)
    }
}
