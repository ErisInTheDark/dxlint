//! What every type-layer question asks of its world: the files it can see,
//! and how they resolve, read and share.
//!
//! The question data itself (leaves, nodes, frames, facts, budget) lives in
//! [`dx_names::question`]; this module adds the [`World`] the evaluator asks.

pub use dx_names::question::*;

use crate::names::{Names, Resolved, ScopeIndex};

/// The files a question can see, for every type-layer engine: file 0 is
/// the linted file; others come in as declarations or module resolution
/// reach them (the engine owns reading, parsing and recorded reads).
pub trait World {
    fn grammar(&self) -> &dx_grammar::Grammar;
    fn profile(&self) -> Option<&dx_core::profile::Profile> { None }
    fn revision(&self) -> u64 { 0 }
    fn path(&self, _file: u32) -> Option<&std::path::Path> { None }
    fn tree(&self, file: u32) -> Option<&dx_tree::Tree>;
    fn text(&self, file: u32) -> Option<&str>;
    /// Whether the file's text spells `needle` anywhere.
    fn text_mentions(&self, file: u32, needle: &str) -> Option<bool> {
        Some(memchr::memmem::find(self.text(file)?.as_bytes(), needle.as_bytes()).is_some())
    }
    /// The file's resolved names (scopes, declarations, references).
    fn names(&self, file: u32) -> Option<&Resolved>;
    /// The file's declarations by scope and name, keyed by its front end.
    fn index(&self, file: u32) -> Option<&ScopeIndex>;
    fn module_index(&self, file: u32, tables: &Names) -> Option<&crate::names::modules::index::Index> {
        Some(crate::names::modules::index::module_index(self.tree(file)?, tables, self.text(file)?, &|text| self.name_key(file, text), self.profile()))
    }
    /// Records that the answer being worked out depends on `file`'s whole
    /// text: for files a scan reached that turn out to matter.
    fn depends_on_file(&self, _file: u32) {}
    /// Records that the answer read part `read` of another file `file`
    /// ([`dx_core::parts`]).
    fn note_part(&self, _file: u32, _read: dx_core::parts::Read) {}
    /// Work is stepping onto `at`: inside another file's body, it depends on
    /// that file's whole text, not only what lies outside its bodies.
    fn note_entered(&self, _at: Leaf) {}
    /// Turns [`Self::note_entered`] off (or back on) for a whole-file syntax
    /// scan, which steps through every body although its answers only
    /// matter where they reach; returns the previous setting.
    fn quiet_entering(&self, _quiet: bool) -> bool { false }
    /// How often the linted file's own syntax was read so far, which no
    /// recorded read covers: answers that read it hold for this text only.
    fn linted_reads(&self) -> u64 { 0 }
    /// Counts a read of the linted file's syntax: a kept answer that read it
    /// reads it again when used.
    fn note_linted_read(&self) {}
    /// What a name's text means in `file` (its front end's escapes).
    fn name_key<'s>(&self, file: u32, text: &'s str) -> std::borrow::Cow<'s, str>;
    /// The scope a module specifier names, asked by node `at` (its file's
    /// resolution profile and modes decide; a file's root scope or an
    /// ambient module); `Yes(None)` when nothing matches.
    fn module(&self, at: Leaf, specifier: &str, semantic: &crate::semantic::Semantic<'_>) -> Known<Option<ScopeRef>>;
    /// Option `name` of the configuration the program around `at` loads
    /// (its resolution profile's config `options`); null when unset.
    fn program_option(&self, _at: Leaf, _name: &str, _semantic: &crate::semantic::Semantic<'_>) -> Known<dx_ir::value::Value<'static>> {
        Known::Unknown
    }
    /// What the root config's command `name` prints for `args`, as a
    /// `{ state, output, message }` record (the engine runs it and records
    /// its inputs).
    fn command_output(&self, _name: &str, _args: &[String]) -> Known<dx_ir::value::Value<'static>> {
        Known::Unknown
    }
    /// The directory of the configuration the program around `at` loads
    /// (`/`-separated); null when it loads none.
    fn program_directory(&self, _at: Leaf, _semantic: &crate::semantic::Semantic<'_>) -> Known<dx_ir::value::Value<'static>> {
        Known::Unknown
    }
    /// What imports `at`'s file, as a `{ importers, is_stop, reaches_stop,
    /// reaches_entry }` record: null when it is not in the project's program.
    fn module_importers(&self, _at: Leaf, _stop: Box<[Box<str>]>, _ignore: Box<[Box<str>]>, _semantic: &crate::semantic::Semantic<'_>) -> Known<dx_ir::value::Value<'static>> {
        Known::Unknown
    }
    /// Who reads each name `at`'s file publishes, as a `{ is_stop, is_entry, is_ignored,
    /// exports: [{ name, importers, reaches_stop, reaches_entry }] }` record:
    /// null when it is not in the project's program.
    fn module_export_users(&self, _at: Leaf, _stop: Box<[Box<str>]>, _ignore: Box<[Box<str>]>, _semantic: &crate::semantic::Semantic<'_>) -> Known<dx_ir::value::Value<'static>> {
        Known::Unknown
    }
    /// Every scope merged into module scope `scope`, canonical first:
    /// ambient `declare module "x"` across files and augmentations of a
    /// file module. Just `scope` where nothing merges.
    fn module_scopes(&self, scope: ScopeRef, _semantic: &crate::semantic::Semantic<'_>) -> Known<Vec<ScopeRef>> {
        Known::Yes(vec![scope])
    }
    fn module_star_excludes(&self, _scope: ScopeRef, _name: &str, _semantic: &crate::semantic::Semantic<'_>) -> Known<bool> {
        Known::Unknown
    }
    /// Where program file `file` comes from (its path under the program's
    /// resolution profile).
    fn origin(&self, _file: u32) -> Known<crate::names::modules::resolution::Origin> {
        Known::Unknown
    }
    /// The global scopes the program around file `importer` sees (libs,
    /// type roots, script files, global augmentations), in policy order;
    /// a world may leave out files that cannot publish global `name`.
    fn global_scopes(&self, _importer: u32, _name: &str, _semantic: &crate::semantic::Semantic<'_>) -> Known<Vec<ScopeRef>> {
        Known::Unknown
    }
    /// The answer to `question`, shared with the other worlds that ask it
    /// under the same program; `compute` answers it in this world.
    fn shared_modules(&self, _question: crate::modules::SharedQuestion<'_>, compute: &mut dyn FnMut() -> Known<Vec<Target>>) -> Known<Vec<Target>> {
        compute()
    }
    /// The value of canonical `leaf` (a node in another file than the
    /// linted one), shared with the other worlds of the run that read it;
    /// `compute` evaluates it here.
    fn shared_value(&self, _leaf: Leaf, _semantic: &crate::semantic::Semantic<'_>, compute: &mut dyn FnMut() -> NodeValue) -> NodeValue {
        compute()
    }
    /// Another file's tested roots ([`crate::narrowing`]), shared with the
    /// other worlds of the run: a whole-file scan each would repeat.
    fn shared_tested_roots(&self, _file: u32, _semantic: &crate::semantic::Semantic<'_>, compute: &mut dyn FnMut() -> Option<Vec<(NodeRef, Vec<u32>)>>) -> Option<Vec<(NodeRef, Vec<u32>)>> {
        compute()
    }
    /// Fact `key`, shared with the other worlds of the run that would derive
    /// the same; `compute` derives it here (`None`: unknown, never shared).
    fn shared_fact(&self, _key: &FactKey, _semantic: &crate::semantic::Semantic<'_>, compute: &mut dyn FnMut() -> Option<Fact>) -> Option<Fact> {
        compute()
    }
    /// The value another world of the run already shared for canonical
    /// `leaf`, without computing it.
    fn shared_value_kept(&self, _leaf: Leaf, _semantic: &crate::semantic::Semantic<'_>) -> Option<NodeValue> {
        None
    }
    /// Whether the current evaluation has read an unknown answer (a strict
    /// query then discards its own); `None` outside one.
    fn unknown_read(&self) -> Option<bool> { None }
    /// Resets [`Self::unknown_read`] to `read`: unknowns met by work whose
    /// answer was discarded (an inference input skipped as undecidable) did
    /// not shape the answer.
    fn forget_unknown_reads(&self, _read: bool) {}
    /// Whether item condition `condition` ([`Names::conditions`])
    /// holds at `at` (run on the VM in that node's file; nested questions
    /// see its frame and view).
    fn condition(&self, at: Leaf, condition: u32, semantic: &crate::semantic::Semantic<'_>) -> Known<bool>;
    /// Whether `condition` fails at `at` on syntax alone (its syntax-only
    /// conjuncts are false): false in every frame, decided without a type
    /// question. `false` when that can't be told.
    fn refuted_by_syntax(&self, _at: Leaf, _condition: u32) -> bool { false }
    /// The value of item expression `operand` ([`Names::operands`])
    /// at `at`, run on the VM in that node's file like [`World::condition`].
    fn operand(&self, at: Leaf, operand: u32, semantic: &crate::semantic::Semantic<'_>) -> Known<dx_ir::value::Value<'static>>;
    /// `let` attribute `attr` (engine attribute id, as in
    /// [`crate::names::ATTR_STEP`] path steps) at `at`, run on the VM in that
    /// node's file; `Yes(Null)` when the node's layer has none.
    fn attribute(&self, at: Leaf, attr: u16, semantic: &crate::semantic::Semantic<'_>) -> Known<dx_ir::value::Value<'static>>;
}
