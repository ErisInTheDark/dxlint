//! The TypeScript/JavaScript front end as the engine sees it
//! ([`crate::engine::frontend::Frontend`]): syntax and tokens only.

use std::any::Any;
use std::path::Path;
use std::sync::{Arc, LazyLock};

use super::file::TsFile;
use super::kinds::{field_shape, Kind, ALL_FIELDS, ALL_KINDS, KINDS};
use super::syntax::Syntax;
use super::tokens::Lexed;
use crate::engine::frontend::{FieldShape, FileInput, Frontend, Native, Parsed};

pub struct Ts;

pub static TS: Ts = Ts;

static KIND_NAMES: LazyLock<Vec<&'static str>> = LazyLock::new(|| KINDS.iter().map(|(name, _)| *name).collect());

impl Frontend for Ts {
    fn name(&self) -> &'static str {
        "ts"
    }

    fn kinds(&self) -> &'static [&'static str] {
        &KIND_NAMES
    }

    fn fields(&self) -> &'static [&'static str] {
        super::kinds::FIELD_NAMES
    }

    fn kind_fields(&self, kind: u16) -> Vec<u16> {
        KINDS[usize::from(kind)].1.iter().map(|&field| field as u16).collect()
    }

    fn token_field(&self) -> u16 {
        super::kinds::Field::Token as u16
    }

    fn is_token(&self, kind: u16) -> bool {
        Kind::is_token_index(usize::from(kind))
    }

    fn is_literal_text(&self, kind: u16) -> bool {
        ALL_KINDS[usize::from(kind)].is_literal_text()
    }

    fn root_kind(&self) -> u16 {
        Kind::Program as u16
    }

    fn field_shape(&self, kind: u16, field: u16) -> FieldShape {
        let shape = field_shape(ALL_KINDS[usize::from(kind)], ALL_FIELDS[usize::from(field)]);
        FieldShape { kinds: shape.kinds.iter().map(|&kind| kind as u16).collect(), text: shape.text, any: shape.any }
    }

    fn accepts(&self, path: &Path) -> bool {
        super::reads(path).is_some()
    }

    fn parse(&self, text: &str, path: &Path) -> Result<Parsed, (u32, String)> {
        let parsed = super::parse(text, super::reads(path).unwrap_or(false))?;
        Ok(Parsed { nodes: parsed.nodes, root: parsed.root, state: None })
    }

    fn attach_tokens(&self, text: &str, parsed: &mut Parsed, wanted: &[bool]) {
        let mut mask = [false; ALL_KINDS.len()];
        for (slot, &want) in mask.iter_mut().zip(wanted) {
            *slot = want;
        }
        let mut own = super::Parsed { nodes: std::mem::take(&mut parsed.nodes), root: parsed.root, tokens: None };
        super::tokens::attach_selected(text, &mut own, &mask);
        parsed.nodes = own.nodes;
        // The token stream the attachment scanned, reused by the file.
        parsed.state = own.tokens.map(|tokens| Arc::new(tokens) as Arc<dyn Any + Send + Sync>);
    }

    fn file<'t>(&'t self, input: FileInput<'t, '_>) -> Box<dyn Native + 't> {
        let tokens = input.state.as_deref().and_then(|state| state.downcast_ref::<Lexed>()).cloned();
        Box::new(TsFile::new(Syntax::new(input.tree, input.text, input.front)).with_tokens(tokens))
    }
}
