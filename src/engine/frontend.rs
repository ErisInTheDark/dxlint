//! Native front ends: parsers for syntax a pattern grammar cannot own
//! (context-sensitive syntax, scopes, types), and the facts they answer.
//! The engine knows them only through [`Frontend`]: their node kinds and
//! fields, which files they read, parsing, and one fact object per parsed
//! file. `native <name>` in a config enables one.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::expr::facts::{FactInfo, LinkInfo, ProjectionInfo};
use crate::expr::Value;
use crate::grammar::{NativeFront, Node, Tree};
use super::reads::{Dep, Discovery, Reads};

/// What one parsed file answers about its nodes. Facts, projections and
/// links are numbered as its front end declares them
/// ([`Frontend::facts`]); [`crate::expr::facts`] reads them.
pub trait Native {
    /// Its front end's name.
    fn frontend(&self) -> &'static str;
    fn fact(&self, fact: usize, node: u32) -> Value<'static>;
    /// One field of a projection's fact (`Some`), or whether it is there.
    fn projection(&self, projection: usize, node: u32, field: Option<usize>) -> Value<'static>;
    fn link(&self, link: usize, node: u32) -> Vec<u32>;
    /// Byte `at` lies strictly inside one token or comment (a line of a
    /// multi-line string or block comment starts there).
    fn inside_token(&self, at: usize) -> bool;
    /// What a name's text means for name resolution (escapes decoded).
    fn name_key<'s>(&self, text: &'s str) -> std::borrow::Cow<'s, str> {
        std::borrow::Cow::Borrowed(text)
    }
    /// The engine's resolved names for this file ([`crate::grammar::names`]),
    /// given before the first fact read that needs them.
    fn set_names(&self, _names: std::rc::Rc<crate::grammar::names::Resolved>) {}
}

/// Which node kinds may fill a field of one node kind.
#[derive(Default)]
pub struct FieldShape {
    /// Front-end kind numbers.
    pub kinds: Vec<u16>,
    /// Plain text can fill it.
    pub text: bool,
    /// Anything can (the front end does not say).
    pub any: bool,
}

/// A parsed file: nodes numbered by the front end's own kinds and fields
/// ([`NativeFront::remap`] turns them into grammar ids), and whatever the
/// front end keeps for its fact object.
pub struct Parsed {
    pub nodes: Vec<Node>,
    pub root: u32,
    pub state: Option<Arc<dyn Any + Send + Sync>>,
}

/// One file's facts are built from these.
pub struct FileInput<'t, 'o> {
    pub tree: &'t Tree,
    pub text: &'t str,
    pub front: &'t NativeFront,
    pub path: &'t Path,
    pub project: &'t dyn Project,
    /// Language options (`language ts { globals ... }`) for this file.
    pub options: &'o [(&'o str, &'o Value<'static>)],
    pub state: Option<Arc<dyn Any + Send + Sync>>,
}

pub trait Frontend: Sync {
    /// What `native <name>` enables (`ts`).
    fn name(&self) -> &'static str;

    /// Node kind names, by kind number: the layers it defines.
    fn kinds(&self) -> &'static [&'static str];
    /// Field names, by field number.
    fn fields(&self) -> &'static [&'static str];
    /// The fields a node kind has.
    fn kind_fields(&self, kind: u16) -> Vec<u16>;
    /// The field holding a node's own punctuation and keywords.
    fn token_field(&self) -> u16;
    /// Token nodes (punctuation, keywords), built only when rules need them.
    fn is_token(&self, kind: u16) -> bool;
    /// Literal text: no code (and so no comment) inside.
    fn is_literal_text(&self, kind: u16) -> bool;
    /// The kind of a whole file's root node.
    fn root_kind(&self) -> u16;
    fn field_shape(&self, kind: u16, field: u16) -> FieldShape;

    /// Whether this front end reads `path`.
    fn accepts(&self, path: &Path) -> bool;
    /// Parses a file, or says where (byte offset) and why it could not.
    fn parse(&self, text: &str, path: &Path) -> Result<Parsed, (u32, String)>;
    /// Adds token nodes of the `wanted` kinds (by kind number).
    fn attach_tokens(&self, text: &str, parsed: &mut Parsed, wanted: &[bool]);
    /// The fact object of one parsed file.
    fn file<'t>(&'t self, input: FileInput<'t, '_>) -> Box<dyn Native + 't>;
    /// The facts its files answer, numbered as [`Native::fact`] reads them.
    fn facts(&self) -> Vec<FactInfo> {
        Vec::new()
    }
    fn projections(&self) -> Vec<ProjectionInfo> {
        Vec::new()
    }
    fn links(&self) -> Vec<LinkInfo> {
        Vec::new()
    }

    /// State shared by every file of a project (other files' exports, globals).
    fn project(&self) -> Box<dyn Project>;
    /// Its directory searches ([`super::reads::discover`]), so recorded
    /// results can be checked again.
    fn discoveries(&self) -> &'static [&'static Discovery] {
        &[]
    }
}

/// A front end's project state: what other files said, kept across files
/// and invalidated when they change.
pub trait Project: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn set_discovery_jobs(&mut self, jobs: usize);
    /// Whether a recorded read still holds.
    fn holds(&self, dep: &Dep) -> bool;
    /// Forgets what the `changed` files said; the result tells
    /// [`Project::is_stale`] which recorded reads they broke.
    fn invalidate(&self, changed: &[PathBuf]) -> Box<dyn Any + Send + Sync>;
    fn is_stale(&self, stale: &(dyn Any + Send + Sync), reads: &Reads) -> bool;
}

/// What changed files broke, per front end ([`super::Linter::invalidate`]).
pub struct Stale {
    fronts: Vec<Box<dyn Any + Send + Sync>>,
    pub(super) changed: Vec<PathBuf>,
}

/// Every front end this build has.
pub fn all() -> &'static [&'static dyn Frontend] {
    crate::FRONTENDS
}

pub fn named(name: &str) -> Option<&'static dyn Frontend> {
    all().iter().copied().find(|front| front.name() == name)
}

/// One project state per enabled front end.
pub(super) struct Projects(Vec<Box<dyn Project>>);

impl Projects {
    pub(super) fn new(fronts: &[NativeFront]) -> Self {
        Projects(fronts.iter().map(|front| front.frontend.project()).collect())
    }

    pub(super) fn get(&self, front: usize) -> &dyn Project {
        &*self.0[front]
    }

    pub(super) fn set_discovery_jobs(&mut self, jobs: usize) {
        for project in &mut self.0 {
            project.set_discovery_jobs(jobs);
        }
    }

    pub(super) fn holds(&self, dep: &Dep) -> bool {
        self.0.iter().all(|project| project.holds(dep))
    }

    pub(super) fn invalidate(&self, changed: &[PathBuf]) -> Stale {
        Stale { fronts: self.0.iter().map(|project| project.invalidate(changed)).collect(), changed: changed.to_vec() }
    }

    pub(super) fn is_stale(&self, stale: &Stale, reads: &Reads) -> bool {
        self.0.iter().zip(&stale.fronts).any(|(project, stale)| project.is_stale(&**stale, reads))
    }
}
