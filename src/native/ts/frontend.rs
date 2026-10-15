//! The TypeScript/JavaScript front end as the engine sees it
//! ([`crate::engine::frontend::Frontend`]).

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use super::environment::Environment;
use super::facts::NativeFile;
use super::kinds::{field_shape, Kind, ALL_FIELDS, ALL_KINDS, KINDS};
use super::modules::{Project, Stale};
use super::reads::{Dep, Discovery, Reads};
use super::syntax::Syntax;
use super::tokens::Lexed;
use super::topics;
use crate::engine::frontend::{self, FieldShape, FileInput, Frontend, Native, Parsed};
use crate::expr::facts::{FactInfo, ProjectionInfo};
use crate::expr::Value;

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
        // The token stream the attachment scanned, reused by the file's facts.
        parsed.state = own.tokens.map(|tokens| Arc::new(tokens) as Arc<dyn Any + Send + Sync>);
    }

    fn file<'t>(&'t self, input: FileInput<'t, '_>) -> Box<dyn Native + 't> {
        let project = input.project.as_any().downcast_ref::<Project>().expect("a ts project");
        let tokens = input.state.as_deref().and_then(|state| state.downcast_ref::<Lexed>()).cloned();
        Box::new(
            NativeFile::new(Syntax::new(input.tree, input.text, input.front), input.path, project)
                .with_environment(Environment::from_values(input.options.iter().copied()))
                .with_tokens(tokens),
        )
    }

    fn facts(&self) -> Vec<FactInfo> {
        topics::FACTS.iter().map(|f| FactInfo { name: f.name, doc: f.doc, shape: f.shape }).collect()
    }

    fn projections(&self) -> Vec<ProjectionInfo> {
        topics::PROJECTIONS.iter().map(|p| ProjectionInfo { name: p.name, fields: p.fields }).collect()
    }

    fn project(&self) -> Box<dyn frontend::Project> {
        Box::new(Project::default())
    }

    fn discoveries(&self) -> &'static [&'static Discovery] {
        &DISCOVERIES
    }
}

static DISCOVERIES: [&Discovery; 1] = [&super::reads::GLOBAL_FILES];

impl Native for NativeFile<'_> {
    fn frontend(&self) -> &'static str {
        TS.name()
    }

    fn fact(&self, fact: usize, node: u32) -> Value<'static> {
        (topics::FACTS[fact].get)(self, node)
    }

    fn projection(&self, projection: usize, node: u32, field: Option<usize>) -> Value<'static> {
        (topics::PROJECTIONS[projection].get)(self, node, field)
    }

    fn inside_token(&self, at: usize) -> bool {
        use super::lexer::Tok;
        let tokens = self.cached(super::tokens::lex_comments);
        let after = tokens.0.partition_point(|token| (token.start as usize) < at);
        // JSX text is content laid out by its own lines, not one token.
        after.checked_sub(1).map(|i| &tokens.0[i]).is_some_and(|token| token.end as usize > at && matches!(token.tok, Tok::Template | Tok::Str | Tok::P("/*")))
    }

    fn tokens(&self) -> Option<std::rc::Rc<frontend::Tokens>> {
        Some(self.cached(|file| frontend::Tokens(file.cached(super::tokens::lex).0.iter().map(|token| (token.start, token.end)).collect())))
    }

    fn name_key<'s>(&self, text: &'s str) -> std::borrow::Cow<'s, str> {
        super::bind::decoded_name(text)
    }

    fn set_names(&self, names: std::sync::Arc<crate::grammar::names::Resolved>) {
        NativeFile::set_names(self, names);
    }
}

impl frontend::Project for Project {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn set_discovery_jobs(&mut self, jobs: usize) {
        Project::set_discovery_jobs(self, jobs);
    }

    fn holds(&self, dep: &Dep) -> bool {
        Project::holds(self, dep)
    }

    fn invalidate(&self, changed: &[PathBuf]) -> Box<dyn Any + Send + Sync> {
        Box::new(Project::invalidate(self, changed))
    }

    fn is_stale(&self, stale: &(dyn Any + Send + Sync), reads: &Reads) -> bool {
        stale.downcast_ref::<Stale>().is_some_and(|stale| Project::is_stale(self, stale, reads))
    }
}
