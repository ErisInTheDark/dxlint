//! Names a language declares with `scope`, `declares` and `resolves` items:
//! which nodes open scopes, which declare
//! names into which enclosing scope, and which use a name. [`Names`] holds
//! those tables per matcher; [`Names::resolve`] answers them for one tree.
//!
//! Classes are bit positions: declaration classes (`value`, `type`) are
//! namespaces, scope classes (`function`, `block`) say where a declaration
//! lands. Both come from the items that name them.

pub mod calls;
pub mod effects;
pub mod modules;
pub mod question;
pub mod values;

mod build;
mod resolve;

pub(crate) use build::{class_bits, intern, lower_path, PathOf};
pub use resolve::{NameKey, ReferenceLookup, ResolveOperand, Resolved, ScopeIndex, ScopeInfo, Scopes, Table};

/// One `declares` item.
#[derive(Clone, Debug)]
pub struct Declare {
    /// Declaration classes (bits).
    pub classes: u64,
    /// Labels from the declaring node to the declared names.
    pub path: Vec<u16>,
    /// Scope classes (bits) of `in nearest`.
    pub nearest: u64,
    /// Visible in the whole scope, not only after its position.
    pub hoisted: bool,
    /// `in enclosing`: the search starts outside the scope the node opens.
    pub enclosing: bool,
    /// `declares value "arguments"`: this name, declared by the node itself.
    pub implicit: Option<Box<str>>,
    /// `when` condition ([`Names::conditions`]).
    pub when: Option<u32>,
    pub key: Option<u32>,
    pub filter: Option<u32>,
}

/// One `resolves` item.
#[derive(Clone, Debug)]
pub struct Resolve {
    /// Declaration classes (bits) looked up.
    pub classes: u64,
    /// Labels from the node to the using name; empty: the node itself.
    pub path: Vec<u16>,
    pub when: Option<u32>,
    pub key: Option<u32>,
    pub outside: Option<u32>,
}

pub use values::compile::Value;

/// One `unwraps` item: what the node stands for (no effect), or what
/// evaluating it does.
#[derive(Clone, Debug, Default)]
pub struct Unwrapping {
    pub classes: u64,
    /// Index into [`Vocabulary::effects`].
    pub effect: Option<u16>,
    /// `maybe`: the effect may happen, or the node completes normally.
    pub possible: bool,
    pub value: Option<Value>,
    /// Scope classes of `nearest` (`returned nearest function`).
    pub nearest: u64,
    pub when: Option<u32>,
    /// The item it came from (index in the config's name items), for
    /// conflict errors and runtime defects.
    pub item: u32,
    /// Where that item's layer is named in the config.
    pub span: Option<dx_config::source::Span>,
}

/// One effect on a field path, for the effects names already answer
/// (writes: `assigned`, `mutated`, user effects).
#[derive(Clone, Debug)]
pub struct Effect {
    /// Index into [`Names::effects`].
    pub effect: u16,
    pub path: Vec<u16>,
    pub when: Option<u32>,
    pub value: Option<std::sync::Arc<Value>>,
    pub possible: bool,
    pub item_id: Option<u32>,
}

/// One `scope` item.
#[derive(Clone, Debug)]
pub struct OpenScope {
    /// Scope classes (bits).
    pub classes: u64,
    /// The fields it covers; empty: the whole node.
    pub within: Vec<u16>,
    pub when: Option<u32>,
}

/// What one matcher's nodes do with names.
#[derive(Clone, Debug, Default)]
pub struct KindNames {
    /// Scopes the node opens, each inside the previous.
    pub scopes: Vec<OpenScope>,
    pub declares: Vec<Declare>,
    pub resolves: Vec<Resolve>,
    pub effects: Vec<Effect>,
    pub unwraps: Vec<Unwrapping>,
    pub widens: Vec<(Value, Option<u32>)>,
    pub defines: Vec<(Value, Option<u32>)>,
    /// `refers`: the place a reference names; without one, its `unwraps`
    /// recipe decides.
    pub refers: Vec<(Value, Option<u32>)>,
    /// Traits (bits) the node directly has, each under its condition.
    pub fits: Vec<(u64, u64, Option<u32>)>,
    pub open_fits: Vec<(u64, Option<u32>)>,
    pub carriers: Vec<values::CarrierRecipe>,
    pub arguments: Vec<(Vec<u16>, Option<u32>)>,
    pub indexed: Vec<values::IndexedRecipe>,
    /// `in <field> propagates <value>`.
    pub propagates: Vec<(Vec<u16>, Value, Option<u32>)>,
    /// Execution shapes (`runs`, `catches`, `runs after`, `labels`).
    pub execution: effects::KindEffects,
    /// `members`: scope classes `from x at key` searches on this kind.
    pub member_defaults: u64,
    /// `targets`: when a declaration or effect targets this node, it
    /// targets what these lead to instead; per group, the first alternative
    /// that leads to anything.
    pub targets: Vec<(Vec<Vec<u16>>, Option<u32>)>,
    /// Module publications and cross-file lookups.
    pub modules: modules::compile::KindModules,
    /// Signatures, invocations, expectations and comparison metadata.
    pub calls: calls::KindCalls,
    /// `generics`: paths to the ordered formal type parameters.
    pub generics: Vec<(Vec<u16>, Option<u32>)>,
    /// `opaque`: paths to bodies other files never read inside.
    pub opaque: Vec<(Vec<u16>, Option<u32>)>,
    pub constraints: Vec<(Value, Option<u32>)>,
    pub defaults: Vec<(Value, Option<u32>)>,
    /// `enumerates <value> [in <views>]`: the alternatives a leaf of this
    /// kind (in those scope classes; 0 = the node itself) spells out where
    /// they are enumerated ([`values::Values::enumerated`]).
    pub enumerates: Vec<(Value, u64, Option<u32>)>,
    pub packs: Vec<(question::ProductShapeId, Option<u32>)>,
    /// `members .. inherits`: scope classes and where inherited members
    /// come from, searched after the kind's own.
    pub member_bases: Vec<(u64, Value, Option<u32>)>,
    pub member_closed: Vec<(u64, Option<u32>)>,
    /// `member`: undeclared members (scope classes, key, value).
    pub member_links: Vec<(u64, Value, Value, Option<u32>)>,
}

/// A `when` condition of one item on one layer; the engine compiles it
/// with the layer's fields and lowers it to the VM ([`Names::conditions`]).
pub struct Condition {
    /// Index of the item in the config's name items.
    pub item: usize,
    pub matcher: u32,
    pub expr: std::sync::OnceLock<dx_ir::expr::CExpr>,
    pub lowered: std::sync::OnceLock<dx_ir::vm::VExpr>,
    /// Its syntax-only conjuncts ([`dx_ir::vm::VExpr::syntax_guard`]).
    pub guard: std::sync::OnceLock<Option<dx_ir::vm::VExpr>>,
}

/// An expression one name item evaluates on its layer's nodes (a module
/// specifier, a literal's identity): compiled with that layer's fields and
/// lowered to the VM like a [`Condition`]; read through the semantic
/// `World::operand`.
pub struct Operand {
    /// Source item or resolution mode.
    pub of: dx_config::ast::OperandOf,
    pub matcher: u32,
    /// Which of the item's expressions ([`dx_config::ast::NameItem::operand`]).
    pub part: u8,
    pub expr: std::sync::OnceLock<dx_ir::expr::CExpr>,
    pub lowered: std::sync::OnceLock<dx_ir::vm::VExpr>,
}

/// The class and effect names items use; bits and indexes count into these.
#[derive(Default, Debug)]
pub struct Vocabulary {
    pub declaration_classes: Vec<String>,
    pub scope_classes: Vec<String>,
    /// Effect names (`assigned`, `mutated`, user participles).
    pub effects: Vec<String>,
    /// Trait names (`fits`, `excludes`, `could` / `always`); at most 64.
    pub traits: Vec<String>,
    /// Per trait index, every trait it implies (itself included), closed
    /// over chains. Shorter than `traits` when later traits imply nothing.
    pub implied: Vec<u64>,
    /// Per trait index, the other traits that imply it (no entry when none).
    pub implying: Vec<u64>,
    pub products: Vec<String>,
}

impl Vocabulary {
    /// The bit of declaration class `name` (0 when no item names it).
    pub fn declaration_class(&self, name: &str) -> u64 {
        self.declaration_classes.iter().position(|c| c == name).map_or(0, |i| 1 << i)
    }
    pub fn scope_class(&self, name: &str) -> u64 {
        self.scope_classes.iter().position(|c| c == name).map_or(0, |i| 1 << i)
    }
    pub fn effect(&self, name: &str) -> Option<u16> {
        self.effects.iter().position(|e| e == name).map(|i| i as u16)
    }
    pub fn trait_bit(&self, name: &str) -> u64 {
        self.traits.iter().position(|t| t == name).map_or(0, |i| 1 << i)
    }
    /// `bits` with every trait they imply.
    pub fn with_implied(&self, bits: u64) -> u64 {
        self.implied.iter().enumerate().filter(|&(index, _)| bits & (1 << index) != 0).fold(bits, |all, (_, &implied)| all | implied)
    }
}

/// The tables, per matcher (empty when the config has no name items).
#[derive(Default)]
pub struct Names {
    pub effect_roles: Vec<effects::EffectRole>,
    pub vocabulary: std::sync::Arc<Vocabulary>,
    pub kinds: Vec<KindNames>,
    pub conditions: Vec<Condition>,
    /// Every item expression evaluated on nodes ([`Operand`]).
    pub operands: Vec<Operand>,
    /// `exclusive` groups: trait bits no possible value has together.
    pub exclusive: Vec<u64>,
    pub top: u64,
    pub dynamic: u64,
    pub key_domains: Vec<(dx_config::ast::values::KeyDomain, u64)>,
    /// `keys truthiness`: the trait bits a truthy and a falsy key literal fit.
    pub key_truthiness: Option<(u64, u64)>,
    /// `skip narrowing`: reads keep their declared value.
    pub skip_narrowing: bool,
    /// `skip selection`: conditional selections (`select`) are unknown.
    pub skip_selection: bool,
    pub products: Vec<Option<values::products::Shape>>,
    /// `apparent <trait> inherits <value>`: the trait's bit and the value
    /// whose members values with it read through.
    pub apparent: Vec<(u64, values::compile::Value)>,
    /// `module_resolution` profiles, compiled.
    pub module_resolutions: Vec<modules::compile::Profile>,
    /// Every `let` attribute a semantic recipe path steps through: what
    /// files read only for type questions must still be able to compute.
    pub recipe_attributes: Vec<u16>,
    /// Every field label a semantic recipe path steps through.
    pub recipe_fields: Vec<u16>,
}

impl Names {
    /// The place of the resolution profile `settings` (its own or a mode's)
    /// belongs to: its name across runs of these rules, unlike an address.
    pub fn settings_index(&self, settings: &modules::resolution::Settings) -> Option<u32> {
        self.module_resolutions.iter().position(|profile| std::ptr::eq(&profile.settings, settings)
            || profile.modes.iter().any(|mode| std::ptr::eq(&mode.settings, settings))).and_then(|at| u32::try_from(at).ok())
    }

    /// The settings of the profile at [`Names::settings_index`] `index`.
    pub fn settings_at(&self, index: u32) -> Option<&modules::resolution::Settings> {
        self.module_resolutions.get(index as usize).map(|profile| &profile.settings)
    }
}

pub use dx_tree::steps::*;

/// Steps that ask a value question (`result`, `expected`, `inferred`, ...)
/// instead of leading to syntax sites.
pub fn query_step(step: u16) -> bool {
    matches!(step, RESULT | EXPECTED | INFERRED | RETURNED | WIDENED | RECEIVER | FIRST_PARAMETER)
}

pub fn attribute_step(step: u16) -> Option<u16> {
    (ATTR_STEP..SPECIAL_STEPS).contains(&step).then_some(step & !ATTR_STEP)
}

/// No class, no effect: what a compile path with no names uses.
pub static EMPTY_VOCABULARY: Vocabulary = Vocabulary {
    declaration_classes: Vec::new(),
    scope_classes: Vec::new(),
    effects: Vec::new(),
    traits: Vec::new(),
    implied: Vec::new(),
    implying: Vec::new(),
    products: Vec::new(),
};

#[cfg(test)]
use dx_config::ast::Config;
#[cfg(test)]
use dx_config::error::ConfigError;
#[cfg(test)]
use dx_grammar::Grammar;

/// Builds plain names in tests: no builtin names, no constants, no queries.
#[cfg(test)]
pub(crate) fn plain_env() -> dx_grammar::BuildEnv<'static> {
    dx_grammar::BuildEnv::plain()
}

/// Builds a grammar and its names in tests, with no builtin names and no
/// constants.
#[cfg(test)]
pub(crate) fn build_plain(config: &Config, errors: &mut Vec<ConfigError>) -> (Grammar, Names) {
    let env = plain_env();
    let grammar = Grammar::build(config, errors, &env);
    let names = Names::build(config, &grammar, errors, env.constant);
    (grammar, names)
}
