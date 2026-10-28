//! Names a language declares with `scope`, `declares` and `resolves` items:
//! which nodes open scopes, which declare
//! names into which enclosing scope, and which use a name. [`Names`] holds
//! those tables per matcher; [`Names::resolve`] answers them for one tree.
//!
//! Classes are bit positions: declaration classes (`value`, `type`) are
//! namespaces, scope classes (`function`, `block`) say where a declaration
//! lands. Both come from the items that name them.

use std::borrow::Cow;

use crate::config::ast::{Config, Name, NameItem};

pub mod calls;
pub mod effects;
mod memo;
pub mod modules;
pub mod narrowing;
pub mod question;
pub mod semantic;
pub mod values;
use crate::config::error::ConfigError;
use crate::fx_hash::FxHashMap;

use super::{Grammar, Tree};

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
use values::compile::lower_value;

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
    pub span: Option<crate::config::source::Span>,
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
    /// `opaque`: paths to bodies other files see only by entering them.
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
    pub expr: std::sync::OnceLock<crate::expr::CExpr>,
    pub lowered: std::sync::OnceLock<crate::vm::VExpr>,
    /// Its syntax-only conjuncts ([`crate::vm::VExpr::syntax_guard`]).
    pub guard: std::sync::OnceLock<Option<crate::vm::VExpr>>,
}

/// An expression one name item evaluates on its layer's nodes (a module
/// specifier, a literal's identity): compiled with that layer's fields and
/// lowered to the VM like a [`Condition`]; read through
/// [`question::World::operand`].
pub struct Operand {
    /// Source item or resolution mode.
    pub of: crate::config::ast::OperandOf,
    pub matcher: u32,
    /// Which of the item's expressions ([`NameItem::operand`]).
    pub part: u8,
    pub expr: std::sync::OnceLock<crate::expr::CExpr>,
    pub lowered: std::sync::OnceLock<crate::vm::VExpr>,
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
    /// Closes `trait a implies b, c` items over chains; the names must
    /// already be interned.
    fn imply(&mut self, implications: &[(Name, Vec<Name>)], errors: &mut Vec<ConfigError>) {
        let Some(edges) = implications.iter().map(|(name, implied)| {
            Some((class_bits(&mut self.traits, std::slice::from_ref(name), errors)?, class_bits(&mut self.traits, implied, errors)?))
        }).collect::<Option<Vec<_>>>() else { return };
        self.implied = (0..self.traits.len()).map(|index| 1u64 << index).collect();
        loop {
            let mut changed = false;
            for &(from, to) in &edges {
                let index = from.trailing_zeros() as usize;
                let closed = to | self.with_implied(to);
                if self.implied[index] | closed != self.implied[index] {
                    self.implied[index] |= closed;
                    changed = true;
                }
            }
            if !changed { break }
        }
        self.implying = vec![0; self.traits.len()];
        for (from, &implied) in self.implied.iter().enumerate() {
            for to in 0..self.traits.len() {
                if to != from && implied & (1 << to) != 0 { self.implying[to] |= 1 << from }
            }
        }
    }
}

/// `name`'s index in `list`, adding it.
fn intern(list: &mut Vec<String>, name: &str) -> usize {
    list.iter().position(|x| x == name).unwrap_or_else(|| {
        list.push(name.to_string());
        list.len() - 1
    })
}

/// Resolves a field path to labels, reporting unknown fields.
type PathOf<'a> = dyn Fn(&mut Vec<ConfigError>, &[Name]) -> Option<Vec<u16>> + 'a;

pub(crate) fn lower_path(g: &Grammar, attrs: &crate::expr::AttrNames, errors: &mut Vec<ConfigError>, path: &[Name]) -> Option<Vec<u16>> {
    path.iter().map(|field| match (g.labels.get(&field.text), field.text.as_str()) {
        (Some(label), _) if label < ATTR_STEP => Ok(label),
        (Some(_), _) => {
            errors.push(ConfigError::new(field.span, "too many fields for name item paths"));
            Err(())
        }
        (None, "parent") => Ok(PARENT),
        (None, "it") => Ok(IT),
        (None, "through") => Ok(THROUGH),
        (None, "definition") => Ok(DECLARATION),
        (None, "position") => Ok(POSITION),
        (None, "result") => Ok(RESULT),
        (None, "expected") => Ok(EXPECTED),
        (None, "inferred") => Ok(INFERRED),
        (None, "returned") => Ok(RETURNED),
        (None, "widened") => Ok(WIDENED),
        (None, "receiver") => Ok(RECEIVER),
        (None, "first_parameter") => Ok(FIRST_PARAMETER),
        (None, name) => match attrs.get(name) {
            Some(attr) if attr < SPECIAL_STEPS - ATTR_STEP => Ok(ATTR_STEP | attr),
            _ => {
                errors.push(ConfigError::new(field.span, format!("no layer has a field or attribute `{}`", field.text))
                    .note("path steps are fields, `let` attributes, or `parent`, `it`, `through`, `definition`, `position`, `result`, `expected`, `inferred`, `returned`, `widened`, `receiver`"));
                Err(())
            }
        },
    }).collect::<Result<Vec<_>, _>>().ok()
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
    pub key_domains: Vec<(crate::config::ast::values::KeyDomain, u64)>,
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

/// One tree's resolved names.
#[derive(Default)]
pub struct Resolved {
    pub complete: bool,
    /// What the class bits and effect indexes below mean.
    pub vocabulary: std::sync::Arc<Vocabulary>,
    pub keys: FxHashMap<u32, Option<Box<str>>>,
    pub failures: Vec<(u32, Box<str>)>,
    /// Using name -> the name node that declares it (the first of merged
    /// declarations); a later merged declaration -> the first.
    pub declaration: FxHashMap<u32, u32>,
    /// Later declarations of a merged name (`var a; var a;`, `class X` +
    /// `interface X`) -> the first, which owns the binding.
    pub merged: FxHashMap<u32, u32>,
    /// The first of merged declarations -> the later ones, in order.
    merged_later: FxHashMap<u32, Vec<u32>>,
    /// Declaring name (the first of merged ones) -> using names, in order.
    pub references: FxHashMap<u32, Vec<u32>>,
    /// Node an effect targets (and every `targets` node on the way) ->
    /// the effect ([`Names::effects`]) and the node whose evaluation has it.
    /// The first effect on a node wins.
    pub effects: FxHashMap<u32, (u16, u32)>,
    /// Scopes in document order; the first is the outermost opened.
    pub scopes: Vec<ScopeInfo>,
    /// Declared name -> the scope it lands in, the node whose `declares`
    /// item declares it, its classes and whether it is hoisted.
    pub declared: FxHashMap<u32, (u32, u32, u64, bool)>,
    /// Further scopes the same name also lands in, as in `declared` (a
    /// constructor parameter property: a parameter and a class member).
    /// `declared` keeps the first: the name's own lexical binding.
    pub also_declared: FxHashMap<u32, Vec<(u32, u32, u64, bool)>>,
    /// Semantic descriptors stay separate from declarations' lexical owners.
    pub bindings: FxHashMap<u32, u32>,
    /// Implicit declarations (`declares value "arguments"`): node -> names.
    pub implicit: FxHashMap<u32, Vec<Box<str>>>,
    pub asked: FxHashMap<u32, ReferenceLookup>,
    /// Declarations by scope and name, built on first ask ([`Resolved::index`]).
    index: std::sync::OnceLock<ScopeIndex>,
    /// Node -> the innermost scope it sits in (its own when it opens one),
    /// `within` fields respected; [`NO_SCOPE`] outside every scope.
    scope_of: Vec<u32>,
}

/// What a name's text means as a key (a language's escapes):
/// [`crate::engine::frontend::Frontend::name_key`].
pub type NameKey<'k> = dyn for<'a> Fn(&'a str) -> Cow<'a, str> + 'k;

pub type ResolveOperand<'a> = dyn Fn(u32, u32, Option<u32>) -> Result<crate::expr::Value<'static>, Box<str>> + 'a;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReferenceLookup {
    pub scope: u32,
    pub classes: u64,
    /// Ignore only this owner's scopes, preserving nested locals.
    pub outside: Option<u32>,
}

/// One resolved tree's declarations by scope and name, and the scopes each
/// node opens: what type questions look members up in.
#[derive(Default)]
pub struct ScopeIndex {
    /// Scope -> name key -> declaring nodes in document order (implicit
    /// declarations under their text, see [`Resolved::implicit`]).
    declared: Vec<FxHashMap<Box<str>, Vec<u32>>>,
    /// Node -> the scopes it opens, outermost first.
    opened: FxHashMap<u32, (u32, u32)>,
}

impl ScopeIndex {
    /// The scopes `node` opens, outermost first.
    pub fn opened(&self, node: u32) -> std::ops::Range<u32> {
        self.opened.get(&node).map_or(0..0, |&(first, end)| first..end)
    }

    /// What declares `key` in `scope`, in document order.
    pub fn declared(&self, scope: u32, key: &str) -> &[u32] {
        self.declared.get(scope as usize).and_then(|names| names.get(key)).map_or(&[], Vec::as_slice)
    }

    /// Every name declared in `scope`, with its declarations.
    pub fn names(&self, scope: u32) -> impl Iterator<Item = (&str, &[u32])> {
        self.declared.get(scope as usize).into_iter().flat_map(|names| names.iter().map(|(key, decls)| (&**key, decls.as_slice())))
    }
}

/// One scope of a resolved tree.
#[derive(Clone, Debug)]
pub struct ScopeInfo {
    /// The node opening it.
    pub node: u32,
    pub classes: u64,
    pub parent: Option<u32>,
}

/// A declaration in one scope.
#[derive(Clone, Copy, Debug)]
struct Declared {
    name: u32,
    start: u32,
    classes: u64,
    hoisted: bool,
    /// Declared by a text (`"arguments"`): found only when nothing declares
    /// the name explicitly in the scope.
    implicit: bool,
}

struct Scope<'t> {
    /// Name -> its declarations in document order.
    declared: FxHashMap<Cow<'t, str>, Vec<Declared>>,
}

/// No scope (in the dense node -> scope table).
const NO_SCOPE: u32 = u32::MAX;

impl Names {
    /// The tables of `config`'s name items. Classes come from the items
    /// naming them; one looked up or searched for but never declared or
    /// opened is an error, like an unknown matcher.
    pub fn build(config: &Config, g: &Grammar, errors: &mut Vec<ConfigError>) -> Names {
        let mut names = Names { kinds: vec![KindNames::default(); g.matchers.len()], ..Names::default() };
        if config.names.is_empty() && config.exclusive.is_empty() && config.implications.is_empty() && config.top.is_empty() && config.dynamic.is_empty() && config.key_domains.is_empty() && config.key_truthiness.is_none() && config.skips.is_empty() && config.products.is_empty() && config.apparent.is_empty() {
            return names;
        }
        let mut vocab = Vocabulary::default();
        for shape in &config.products {
            vocab.products.push(shape.name.text.clone());
            class_bits(&mut vocab.traits, &shape.traits, errors);
        }
        for definition in &config.names {
            if let NameItem::Fits { traits, .. } = &definition.item {
                if definition.targets.iter().any(|target| g.matcher(&target.text).is_some()) {
                    class_bits(&mut vocab.traits, traits, errors);
                }
            }
        }
        vocab.imply(&config.implications, errors);
        // Numbered as the engine numbers them (first definition order).
        let attrs = crate::expr::AttrNames::new(config.attrs.iter().map(|d| d.name.text.as_str()));
        let mut declared = 0u64;
        let mut opened = 0u64;
        let mut asked: Vec<(&Name, bool)> = Vec::new();
        let mut requested_classes = Vec::new();
        let recipe_attributes = std::cell::RefCell::new(Vec::new());
        let recipe_fields = std::cell::RefCell::new(Vec::new());
        let record = |path: Option<Vec<u16>>| {
            if let Some(path) = &path {
                recipe_attributes.borrow_mut().extend(path.iter().filter_map(|&step| attribute_step(step)));
                recipe_fields.borrow_mut().extend(path.iter().copied().filter(|&step| step < ATTR_STEP));
            }
            path
        };
        names.products = config.products.iter().map(|shape| {
            values::products::lower_shape(shape, g, &attrs, &record, &mut vocab, errors, &mut requested_classes)
        }).collect();
        for apparent in &config.apparent {
            let Some(bits) = class_bits(&mut vocab.traits, std::slice::from_ref(&apparent.name), errors) else { continue };
            let path = |errors: &mut Vec<ConfigError>, path: &[Name]| record(lower_path(g, &attrs, errors, path));
            if let Some(value) = values::compile::lower_value(&apparent.inherits, &path, &mut vocab, errors, &mut requested_classes) {
                names.apparent.push((bits, value));
            }
        }
        for (item, def) in config.names.iter().enumerate() {
            let targets: Vec<u32> = def
                .targets
                .iter()
                .filter_map(|t| match g.matcher(&t.text) {
                    Some(m) => Some(m),
                    None => {
                        errors.push(ConfigError::new(t.span, format!("unknown matcher `{}`", t.text)).suggest(&t.text, g.matchers.iter().map(|m| m.name.as_str())));
                        None
                    }
                })
                .collect();
            let path = |errors: &mut Vec<ConfigError>, path: &[Name]| -> Option<Vec<u16>> {
                record(lower_path(g, &attrs, errors, path))
            };
            // One condition per target layer: it reads that layer's fields.
            let when: Vec<Option<u32>> = targets
                .iter()
                .map(|&matcher| {
                    def.when.as_ref().map(|_| {
                        names.conditions.push(Condition { item, matcher, expr: std::sync::OnceLock::new(), lowered: std::sync::OnceLock::new(), guard: std::sync::OnceLock::new() });
                        names.conditions.len() as u32 - 1
                    })
                })
                .collect();
            match &def.item {
                NameItem::Scope { classes, within } => {
                    let Some(bits) = class_bits(&mut vocab.scope_classes, classes, errors) else { continue };
                    opened |= bits;
                    let Some(within) = path(errors, within) else { continue };
                    for (&m, &when) in targets.iter().zip(&when) {
                        let scope = OpenScope { classes: bits, within: within.clone(), when };
                        names.add(g, m, |k| k.scopes.push(scope.clone()));
                    }
                }
                NameItem::Declares { classes, path: p, implicit, nearest, enclosing, hoisted, keyed, filter } => {
                    let Some(bits) = class_bits(&mut vocab.declaration_classes, classes, errors) else { continue };
                    let Some(near) = class_bits(&mut vocab.scope_classes, nearest, errors) else { continue };
                    declared |= bits;
                    asked.extend(nearest.iter().map(|n| (n, true)));
                    let Some(p) = path(errors, p) else { continue };
                    let implicit: Option<Box<str>> = implicit.as_deref().map(Into::into);
                    for (&m, &when) in targets.iter().zip(&when) {
                        let key = keyed.as_ref().map(|_| names.operand(item, m, NameItem::KEY_OPERAND));
                        let filter = filter.as_ref().map(|_| names.operand(item, m, NameItem::FILTER_OPERAND));
                        let d = Declare { classes: bits, path: p.clone(), nearest: near, hoisted: *hoisted, enclosing: *enclosing, implicit: implicit.clone(), when, key, filter };
                        names.add(g, m, |k| k.declares.push(d.clone()));
                    }
                }
                NameItem::Targets(groups) => {
                    let mut passes = Vec::new();
                    for group in groups {
                        let paths: Option<Vec<Vec<u16>>> = group.iter().map(|p| path(errors, p)).collect();
                        let Some(paths) = paths else { continue };
                        passes.push(paths);
                    }
                    for (&m, &when) in targets.iter().zip(&when) {
                        names.add(g, m, |k| k.targets.extend(passes.iter().map(|group| (group.clone(), when))));
                    }
                }
                NameItem::Widens { value } => {
                    let Some(value) = lower_value(value, &path, &mut vocab, errors, &mut requested_classes) else { continue };
                    for (&matcher, &when) in targets.iter().zip(&when) {
                        names.add(g, matcher, |kind| kind.widens.push((value.clone(), when)));
                    }
                }
                NameItem::Defines { value } => {
                    let Some(value) = lower_value(value, &path, &mut vocab, errors, &mut requested_classes) else { continue };
                    for (&matcher, &when) in targets.iter().zip(&when) {
                        names.add(g, matcher, |kind| kind.defines.push((value.clone(), when)));
                    }
                }
                NameItem::Refers { value } => {
                    let Some(value) = lower_value(value, &path, &mut vocab, errors, &mut requested_classes) else { continue };
                    for (&matcher, &when) in targets.iter().zip(&when) {
                        names.add(g, matcher, |kind| kind.refers.push((value.clone(), when)));
                    }
                }
                NameItem::Enumerates { value, views } => {
                    let Some(value) = lower_value(value, &path, &mut vocab, errors, &mut requested_classes) else { continue };
                    let Some(views_mask) = class_bits(&mut vocab.scope_classes, views, errors) else { continue };
                    asked.extend(views.iter().map(|name| (name, true)));
                    for (&matcher, &when) in targets.iter().zip(&when) {
                        names.add(g, matcher, |kind| kind.enumerates.push((value.clone(), views_mask, when)));
                    }
                }
                NameItem::Constrains { value } => {
                    let Some(value) = lower_value(value, &path, &mut vocab, errors, &mut requested_classes) else { continue };
                    for (&matcher, &when) in targets.iter().zip(&when) {
                        names.add(g, matcher, |kind| kind.constraints.push((value.clone(), when)));
                    }
                }
                NameItem::Defaults { value } => {
                    let Some(value) = lower_value(value, &path, &mut vocab, errors, &mut requested_classes) else { continue };
                    for (&matcher, &when) in targets.iter().zip(&when) {
                        names.add(g, matcher, |kind| kind.defaults.push((value.clone(), when)));
                    }
                }
                NameItem::Pack(shape) => {
                    let Some(index) = vocab.products.iter().position(|name| *name == shape.text) else {
                        errors.push(ConfigError::new(shape.span, format!("unknown product shape `{}`", shape.text)));
                        continue;
                    };
                    for (&matcher, &when) in targets.iter().zip(&when) {
                        names.add(g, matcher, |kind| kind.packs.push((question::ProductShapeId(index as u32), when)));
                    }
                }
                NameItem::Unwraps { effect, possible, value, nearest, classes } => {
                    requested_classes.extend(classes.iter().cloned());
                    let Some(classes) = class_bits(&mut vocab.declaration_classes, classes, errors) else { continue };
                    let effect = effect.as_ref().map(|e| intern(&mut vocab.effects, &e.text) as u16);
                    let Some(near) = class_bits(&mut vocab.scope_classes, nearest, errors) else { continue };
                    asked.extend(nearest.iter().map(|n| (n, true)));
                    let value = match value {
                        Some(v) => match lower_value(v, &path, &mut vocab, errors, &mut requested_classes) {
                            Some(v) => Some(v),
                            None => continue,
                        },
                        None => None,
                    };
                    for (&m, &when) in targets.iter().zip(&when) {
                        // An effect on a field: the targets it reaches carry it (writes).
                        if let (Some(effect), Some(Value::Path(p))) = (effect, &value) {
                            if vocab.effects[effect as usize] != "read" {
                                let e = Effect { effect, path: p.clone(), when, value: None, possible: *possible, item_id: Some(item as u32) };
                                names.add(g, m, |k| k.effects.push(e.clone()));
                            }
                        }
                        let span = def.targets.first().map(|t| t.span);
                        let u = Unwrapping { classes, effect, possible: *possible, value: value.clone(), nearest: near, when, item: item as u32, span };
                        names.add(g, m, |k| k.unwraps.push(u.clone()));
                    }
                }
                NameItem::Execution(execution) => {
                    let Some(lowered) = effects::compile::lower(execution, &path, &mut vocab, errors, &mut requested_classes) else {
                        continue;
                    };
                    for (&m, &when) in targets.iter().zip(&when) {
                        names.add(g, m, |k| k.push_execution(lowered.clone(), when, item as u32, def.targets[0].span));
                    }
                }
                NameItem::Members { classes, closed, inherits } => {
                    let Some(bits) = class_bits(&mut vocab.scope_classes, classes, errors) else { continue };
                    asked.extend(classes.iter().map(|n| (n, true)));
                    let inherits = match inherits {
                        Some(v) => match lower_value(v, &path, &mut vocab, errors, &mut requested_classes) {
                            Some(v) => Some(v),
                            None => continue,
                        },
                        None => None,
                    };
                    for (&m, &when) in targets.iter().zip(&when) {
                        names.add(g, m, |k| k.member_defaults |= bits);
                        if *closed { names.add(g, m, |k| k.member_closed.push((bits, when))) }
                        if let Some(base) = &inherits {
                            names.add(g, m, |k| k.member_bases.push((bits, base.clone(), when)));
                        }
                    }
                }
                NameItem::Member { classes, key, value } => {
                    let Some(bits) = class_bits(&mut vocab.scope_classes, classes, errors) else { continue };
                    asked.extend(classes.iter().map(|n| (n, true)));
                    let Some(key) = lower_value(key, &path, &mut vocab, errors, &mut requested_classes) else { continue };
                    let Some(value) = lower_value(value, &path, &mut vocab, errors, &mut requested_classes) else { continue };
                    for (&m, &when) in targets.iter().zip(&when) {
                        let link = (bits, key.clone(), value.clone(), when);
                        names.add(g, m, |k| k.member_links.push(link.clone()));
                    }
                }
                NameItem::Fits { traits, views, open } => {
                    // Declared bits only: implied traits match through `Values::fits`
                    // but never make an intersection impossible.
                    let Some(bits) = class_bits(&mut vocab.traits, traits, errors) else { continue };
                    let Some(views_mask) = class_bits(&mut vocab.scope_classes, views, errors) else { continue };
                    asked.extend(views.iter().map(|name| (name, true)));
                    for (&m, &when) in targets.iter().zip(&when) {
                        names.add(g, m, |k| {
                            k.fits.push((bits, views_mask, when));
                            if *open { k.open_fits.push((views_mask, when)) }
                        });
                    }
                }
                NameItem::Propagates { field, value } => {
                    let Some(field) = path(errors, field) else { continue };
                    let Some(value) = lower_value(value, &path, &mut vocab, errors, &mut requested_classes) else { continue };
                    for (&m, &when) in targets.iter().zip(&when) {
                        let p = (field.clone(), value.clone(), when);
                        names.add(g, m, |k| k.propagates.push(p.clone()));
                    }
                }
                NameItem::Resolves { classes, path: p, keyed, outside } => {
                    let Some(bits) = class_bits(&mut vocab.declaration_classes, classes, errors) else { continue };
                    asked.extend(classes.iter().map(|c| (c, false)));
                    let Some(p) = path(errors, p) else { continue };
                    for (&m, &when) in targets.iter().zip(&when) {
                        let key = keyed.as_ref().map(|_| names.operand(item, m, NameItem::KEY_OPERAND));
                        let outside = outside.as_ref().map(|_| names.operand(item, m, NameItem::OUTSIDE_OPERAND));
                        let r = Resolve { classes: bits, path: p.clone(), when, key, outside };
                        names.add(g, m, |k| k.resolves.push(r.clone()));
                    }
                }
                NameItem::Generics(p) => {
                    let Some(p) = path(errors, p) else { continue };
                    for (&m, &when) in targets.iter().zip(&when) {
                        names.add(g, m, |k| k.generics.push((p.clone(), when)));
                    }
                }
                NameItem::Arguments(arguments) => {
                    let Some(arguments) = path(errors, arguments) else { continue };
                    for (&m, &when) in targets.iter().zip(&when) {
                        names.add(g, m, |kind| kind.arguments.push((arguments.clone(), when)));
                    }
                }
                NameItem::Opaque(field) => {
                    let Some(field) = path(errors, field) else { continue };
                    for (&m, &when) in targets.iter().zip(&when) {
                        names.add(g, m, |kind| kind.opaque.push((field.clone(), when)));
                    }
                }
                NameItem::Indexed { path: indexed, mode, value, views, runtime } => {
                    let Some(indexed_path) = path(errors, indexed) else { continue };
                    let mode = match mode {
                        crate::config::ast::values::IndexedMode::Slots { repeated, strings } => values::IndexedMode::Slots { repeated: *repeated, strings: *strings },
                        crate::config::ast::values::IndexedMode::Domain(domain) => values::IndexedMode::Domain(*domain),
                        crate::config::ast::values::IndexedMode::Keyed(key) => {
                            let Some(key) = lower_value(key, &path, &mut vocab, errors, &mut requested_classes) else { continue };
                            values::IndexedMode::Keyed(key)
                        }
                        crate::config::ast::values::IndexedMode::Keys { bind } => {
                            let bind = match bind {
                                Some(bind) => {
                                    let Some(bind) = path(errors, bind) else { continue };
                                    Some(bind)
                                }
                                None => None,
                            };
                            values::IndexedMode::Keys { bind }
                        }
                    };
                    let Some(views) = class_bits(&mut vocab.scope_classes, views, errors) else { continue };
                    let value = match value {
                        Some(value) => {
                            let Some(value) = lower_value(value, &path, &mut vocab, errors, &mut requested_classes) else { continue };
                            Some(value)
                        }
                        None => None,
                    };
                    for (&m, &when) in targets.iter().zip(&when) {
                        names.add(g, m, |kind| kind.indexed.push(values::IndexedRecipe {
                            path: indexed_path.clone(), mode: mode.clone(), views, value: value.clone(), when, runtime: *runtime,
                        }));
                    }
                }
                NameItem::Carries { arguments, value, .. } => {
                    let arguments = match arguments {
                        Some(arguments) => {
                            let Some(arguments) = path(errors, arguments) else { continue };
                            Some(arguments)
                        }
                        None => None,
                    };
                    let value = match value {
                        Some(value) => {
                            let Some(value) = lower_value(value, &path, &mut vocab, errors, &mut requested_classes) else { continue };
                            Some(value)
                        }
                        None => None,
                    };
                    for (&m, &when) in targets.iter().zip(&when) {
                        let description = names.operand(item, m, 0);
                        names.add(g, m, |kind| kind.carriers.push(values::CarrierRecipe {
                            description, arguments: arguments.clone(), value: value.clone(), when,
                        }));
                    }
                }
                NameItem::Calls(call) => {
                    for (&m, &when) in targets.iter().zip(&when) {
                        let lowered = calls::compile::lower(
                            call,
                            &mut |p, errors| path(errors, p),
                            &mut |v, errors| lower_value(v, &path, &mut vocab, errors, &mut requested_classes),
                            &mut |part| names.operand(item, m, part),
                            errors,
                        );
                        let Some(lowered) = lowered else { break };
                        names.add(g, m, |k| k.calls.push(lowered.clone(), when));
                    }
                }
                NameItem::Module(module) => {
                    let scope = matches!(module, crate::config::ast::modules::Item::Scope { .. });
                    let classes = module.classes();
                    let list = if scope { &mut vocab.scope_classes } else { &mut vocab.declaration_classes };
                    let Some(bits) = class_bits(list, classes, errors) else { continue };
                    if scope {
                        opened |= bits;
                    } else {
                        asked.extend(classes.iter().map(|c| (c, false)));
                    }
                    let asks = match module.asking() {
                        None => 0,
                        Some(crate::config::ast::modules::Asking::All) => u64::MAX,
                        Some(crate::config::ast::modules::Asking::Classes(classes)) => {
                            asked.extend(classes.iter().map(|class| (class, false)));
                            let Some(bits) = class_bits(&mut vocab.declaration_classes, classes, errors) else { continue };
                            bits
                        }
                    };
                    for (&m, &when) in targets.iter().zip(&when) {
                        if scope {
                            let open = OpenScope { classes: bits, within: Vec::new(), when };
                            names.add(g, m, |k| k.scopes.push(open.clone()));
                        }
                        let lowered = modules::compile::lower(module, bits, asks, when, &mut |part| names.operand(item, m, part));
                        names.add(g, m, |k| k.modules.items.push(lowered.clone()));
                    }
                }
            }
        }
        for (name, scope) in asked.into_iter().chain(requested_classes.iter().map(|name| (name, false))) {
            let (list, have, what) = if scope {
                (&vocab.scope_classes, opened, "no layer opens a scope of class")
            } else {
                (&vocab.declaration_classes, declared, "nothing declares class")
            };
            let bit = list.iter().position(|c| *c == name.text).map_or(0, |i| 1u64 << i);
            if have & bit == 0 {
                errors.push(ConfigError::new(name.span, format!("{what} `{}`", name.text)).suggest(&name.text, list.iter().map(String::as_str)));
            }
        }
        names.exclusive = values::exclusive_groups(config, &mut vocab, &names, errors);
        names.top = values::algebra_traits(&config.top, &vocab, &names, errors);
        names.dynamic = values::algebra_traits(&config.dynamic, &vocab, &names, errors);
        names.key_domains = config.key_domains.iter().map(|(domain, traits)| {
            (*domain, values::algebra_traits(traits, &vocab, &names, errors))
        }).collect();
        names.skip_narrowing = config.skips.iter().any(|skip| skip.text == "narrowing");
        names.skip_selection = config.skips.iter().any(|skip| skip.text == "selection");
        names.key_truthiness = config.key_truthiness.as_ref().map(|(truthy, falsy)| (
            values::algebra_traits(std::slice::from_ref(truthy), &vocab, &names, errors),
            values::algebra_traits(std::slice::from_ref(falsy), &vocab, &names, errors),
        ));
        names.module_resolutions = modules::compile::profiles(config, g, &vocab, errors, &mut |definition, mode| names.mode_operand(definition, mode));
        names.vocabulary = std::sync::Arc::new(vocab);
        names.effect_roles = effects::compile::effect_roles(&names.vocabulary);
        effects::compile::validate(&names, errors);
        let mut recipe_attributes = recipe_attributes.into_inner();
        recipe_attributes.sort_unstable();
        recipe_attributes.dedup();
        names.recipe_attributes = recipe_attributes;
        let mut recipe_fields = recipe_fields.into_inner();
        recipe_fields.sort_unstable();
        recipe_fields.dedup();
        names.recipe_fields = recipe_fields;
        names
    }

    /// Registers expression `part` of name item `item` on `matcher`'s nodes.
    pub fn operand(&mut self, item: usize, matcher: u32, part: u8) -> u32 {
        self.operands.push(Operand { of: crate::config::ast::OperandOf::Item(item), matcher, part, expr: std::sync::OnceLock::new(), lowered: std::sync::OnceLock::new() });
        self.operands.len() as u32 - 1
    }

    pub fn mode_operand(&mut self, definition: usize, mode: usize) -> u32 {
        let id = self.operands.len() as u32;
        self.operands.push(Operand { of: crate::config::ast::OperandOf::Mode { definition, mode }, matcher: super::ROOT, part: 0, expr: std::sync::OnceLock::new(), lowered: std::sync::OnceLock::new() });
        id
    }

    /// Applies `f` to `m` and every matcher specialising it (`like`).
    fn add(&mut self, g: &Grammar, m: u32, f: impl Fn(&mut KindNames)) {
        for (i, kind) in self.kinds.iter_mut().enumerate() {
            if g.is_a(i as u32, m) {
                f(kind);
            }
        }
    }

    /// Nothing for [`Names::resolve`] to do: no item opens scopes,
    /// declares, uses or writes names (type-layer items are answered on
    /// demand, not here).
    fn resolves_nothing(&self) -> bool {
        self.kinds.iter().all(|k| k.scopes.is_empty() && k.declares.is_empty() && k.resolves.is_empty() && k.effects.is_empty() && k.targets.is_empty())
    }

    /// What a declaration or effect targeting `node` reaches: the node
    /// itself, or what its `targets` paths lead to, recursively. `passed`
    /// collects the nodes passing it on.
    fn targets(&self, tree: &Tree, node: u32, holds: &dyn Fn(u32, Option<u32>) -> bool, out: &mut Vec<u32>, passed: &mut Vec<u32>, active: &mut Vec<u32>) -> bool {
        const MAX_TARGET_DEPTH: usize = 256;
        let Some(node) = tree.semantic_node(node) else { return false };
        if active.len() > MAX_TARGET_DEPTH || active.contains(&node) { return false; }
        let targets = self.kinds.get(tree.node(node).matcher as usize).map_or(&[][..], |k| &k.targets[..]);
        let mut targets = targets.iter().filter(|(_, when)| holds(node, *when)).peekable();
        if targets.peek().is_none() {
            out.push(node);
            return true;
        }
        let (reached, passing) = (out.len(), passed.len());
        active.push(node);
        passed.push(node);
        // Per group, the first alternative that reaches anything: `value ??
        // key` falls back to the key when the value is a pattern reaching
        // nothing (`{ a = 1 }`).
        for (group, _) in targets {
            for path in group {
                let before = out.len();
                let Some(nodes) = tree.semantic_follow(node, path) else {
                    out.truncate(reached);
                    passed.truncate(passing);
                    active.pop();
                    return false;
                };
                for n in nodes {
                    if !self.targets(tree, n, holds, out, passed, active) {
                        out.truncate(reached);
                        passed.truncate(passing);
                        active.pop();
                        return false;
                    }
                }
                if out.len() > before {
                    break;
                }
            }
        }
        if out.len() == reached {
            passed.truncate(passing);
        }
        active.pop();
        true
    }

    /// What a declaration or effect aimed at `node` reaches through
    /// `targets` (the node itself where none apply); unknown when a
    /// `targets` condition cannot be decided.
    pub fn targets_of(&self, tree: &Tree, node: u32, holds: &dyn Fn(u32, u32) -> question::Known<bool>) -> question::Known<Vec<u32>> {
        let unknown = std::cell::Cell::new(false);
        let decide = |at: u32, condition: Option<u32>| {
            condition.is_none_or(|c| match holds(at, c) {
                question::Known::Yes(held) => held,
                question::Known::Unknown => {
                    unknown.set(true);
                    false
                }
            })
        };
        let (mut out, mut passed) = (Vec::new(), Vec::new());
        let complete = self.targets(tree, node, &decide, &mut out, &mut passed, &mut Vec::new());
        if !complete || unknown.get() { question::Known::Unknown } else { question::Known::Yes(out) }
    }

    /// Resolves every name `tree` uses. `key` turns a name's text into what
    /// it means (a language's escapes); `when(node, condition)` evaluates a
    /// `when` condition on a node.
    pub fn resolve<'t>(&self, tree: &Tree, text: &'t str, key: &dyn Fn(&'t str) -> Cow<'t, str>, when: &dyn Fn(u32, u32) -> bool, operand: &ResolveOperand<'_>) -> Resolved {
        let mut out = Resolved { vocabulary: self.vocabulary.clone(), complete: true, ..Resolved::default() };
        if self.resolves_nothing() {
            return out;
        }
        let holds = |node: u32, c: Option<u32>| c.is_none_or(|c| when(node, c));
        let name_key = |id: u32| {
            let n = tree.node(id);
            key(&text[n.start as usize..n.end as usize])
        };
        let mut scopes: Vec<Scope> = Vec::new();
        // Node -> the innermost scope it sits in (its own, when it opens one).
        let mut scope_of = vec![NO_SCOPE; tree.nodes.len()];
        let at = |scope_of: &[u32], id: u32| Some(scope_of[id as usize] as usize).filter(|&s| s != NO_SCOPE as usize);
        let mut uses = Vec::new();
        let mut declared = Vec::new();
        let mut passed = Vec::new();
        // Node opening a scope `within` some fields -> those fields, and the
        // scope its other children sit in.
        let mut limited: FxHashMap<u32, (&[u16], Option<usize>)> = FxHashMap::default();
        for &id in &tree.order {
            let outer = tree.parent(id).and_then(|p| match limited.get(&p) {
                Some(&(within, outside)) if !tree.label(id).is_some_and(|l| within.contains(&l)) => outside,
                _ => at(&scope_of, p),
            });
            let Some(kind) = self.kinds.get(tree.node(id).matcher as usize) else {
                if let Some(outer) = outer {
                    scope_of[id as usize] = outer as u32;
                }
                continue;
            };
            // Each `scope` item opens one scope, inside the previous one
            // (a function expression's name outside its parameters).
            let mut here = outer;
            for s in kind.scopes.iter().filter(|s| holds(id, s.when)) {
                if !s.within.is_empty() && !limited.contains_key(&id) {
                    limited.insert(id, (&s.within, here));
                }
                scopes.push(Scope { declared: FxHashMap::default() });
                out.scopes.push(ScopeInfo { node: id, classes: s.classes, parent: here.map(|s| s as u32) });
                here = Some(scopes.len() - 1);
            }
            if let Some(here) = here {
                scope_of[id as usize] = here as u32;
            }
            for d in kind.declares.iter().filter(|d| holds(id, d.when)) {
                let mut target = if d.enclosing { outer } else { here };
                while let Some(s) = target.filter(|&s| out.scopes[s].classes & d.nearest == 0) {
                    target = out.scopes[s].parent.map(|p| p as usize);
                }
                let Some(target) = target else { continue };
                if let Some(name) = &d.implicit {
                    if d.filter.is_some_and(|expression| !out.evaluate_filter(id, id, expression, operand)) { continue }
                    let name = match d.key {
                        Some(expression) => match out.evaluate_key(id, id, expression, operand) {
                            Some(key) => key,
                            None => continue,
                        },
                        None => name.clone(),
                    };
                    let entry = Declared { name: id, start: tree.node(id).start, classes: d.classes, hoisted: true, implicit: true };
                    scopes[target].declared.entry(Cow::Owned(name.to_string())).or_default().push(entry);
                    out.implicit.entry(id).or_default().push(name);
                    out.declared.insert(id, (target as u32, id, d.classes, true));
                    out.bindings.insert(id, id);
                    continue;
                }
                let Some(named_nodes) = tree.semantic_follow(id, &d.path) else {
                    out.failure(id, "name path cannot be resolved".into());
                    continue;
                };
                for named in named_nodes {
                    declared.clear();
                    passed.clear();
                    if !self.targets(tree, named, &holds, &mut declared, &mut passed, &mut Vec::new()) {
                        out.failure(id, "name path cannot be resolved".into());
                        continue;
                    }
                    let descriptor = if passed.first() == Some(&named) { named } else { id };
                    for &name in &declared {
                        if d.filter.is_some_and(|expression| !out.evaluate_filter(id, name, expression, operand)) { continue }
                        let key = match d.key {
                            Some(expression) => match out.evaluate_key(id, name, expression, operand) {
                                Some(key) => Cow::Owned(key.into()),
                                None => continue,
                            },
                            None => match out.keys.get(&name) {
                                Some(Some(key)) => Cow::Owned(key.to_string()),
                                Some(None) => continue,
                                None => name_key(name),
                            },
                        };
                        let entry = Declared { name, start: tree.node(name).start, classes: d.classes, hoisted: d.hoisted, implicit: false };
                        scopes[target].declared.entry(key).or_default().push(entry);
                        let membership = (target as u32, id, d.classes, d.hoisted);
                        match out.declared.entry(name) {
                            std::collections::hash_map::Entry::Vacant(entry) => { entry.insert(membership); }
                            std::collections::hash_map::Entry::Occupied(mut entry) => {
                                // Same scope: one membership (a second owner there adds nothing).
                                let merge = |(scope, owner, classes, hoisted): &mut (u32, u32, u64, bool)| {
                                    if *scope != membership.0 { return false }
                                    if *owner == membership.1 { *classes |= membership.2; *hoisted |= membership.3; }
                                    true
                                };
                                if !merge(entry.get_mut()) {
                                    let others = out.also_declared.entry(name).or_default();
                                    if !others.iter_mut().any(merge) { others.push(membership) }
                                }
                            }
                        }
                        out.bindings.entry(name).or_insert(descriptor);
                    }
                }
                passed.clear();
            }
            for e in kind.effects.iter().filter(|e| holds(id, e.when)) {
                declared.clear();
                passed.clear();
                let Some(targets) = tree.semantic_follow(id, &e.path) else {
                    out.failure(id, "name path cannot be resolved".into());
                    continue;
                };
                for target in targets {
                    if !self.targets(tree, target, &holds, &mut declared, &mut passed, &mut Vec::new()) {
                        out.failure(id, "name path cannot be resolved".into());
                        declared.clear();
                        passed.clear();
                        break;
                    }
                }
                for &node in passed.iter().chain(&declared) {
                    out.effects.entry(node).or_insert((e.effect, id));
                }
            }
            if here.is_none() && kind.resolves.iter().any(|resolve| resolve.outside.is_some() && holds(id, resolve.when)) {
                out.failure(id, "outside lookup requires an active scope".into());
            }
            if let Some(here) = here {
                for r in kind.resolves.iter().filter(|r| holds(id, r.when)) {
                    if r.path.is_empty() {
                        uses.push((id, here, r.classes, id, r.key, r.outside));
                        continue;
                    }
                    // A use aimed at a qualified name or parentheses passes
                    // on through `targets` (`A.B` in a type looks up `A`).
                    declared.clear();
                    let Some(named_nodes) = tree.semantic_follow(id, &r.path) else {
                        out.failure(id, "name path cannot be resolved".into());
                        continue;
                    };
                    for named in named_nodes {
                        if !self.targets(tree, named, &holds, &mut declared, &mut passed, &mut Vec::new()) {
                            out.failure(id, "name path cannot be resolved".into());
                            declared.clear();
                            break;
                        }
                    }
                    passed.clear();
                    uses.extend(declared.iter().map(|&name| (name, here, r.classes, id, r.key, r.outside)));
                }
            }
        }
        for scope in &scopes {
            for decls in scope.declared.values() {
                let mut hoisted = decls.iter().filter(|d| d.hoisted && !d.implicit);
                if let Some(first) = hoisted.next() {
                    for later in hoisted.filter(|d| d.name != first.name) {
                        out.merged.insert(later.name, first.name);
                        out.merged_later.entry(first.name).or_default().push(later.name);
                    }
                }
            }
        }
        // A name looked up twice keeps the first lookup, found or not: a
        // parent's `resolves type name` comes before the name's own
        // `resolves value`.
        for (name, scope, want, item, expression, outside) in uses {
            if out.asked.contains_key(&name) || out.declared.get(&name).is_some_and(|(_, _, classes, _)| classes & want != 0) {
                continue;
            }
            let mut lookup = ReferenceLookup { scope: scope as u32, classes: want, outside: None };
            out.asked.insert(name, lookup);
            if let Some(expression) = outside {
                let owner = match operand(item, expression, Some(name)) {
                    Ok(crate::expr::Value::Node(owner, _)) => owner,
                    Ok(_) => {
                        out.failure(item, "outside lookup must select a local ancestor opening scopes".into());
                        continue;
                    }
                    Err(message) => {
                        out.failure(item, message);
                        continue;
                    }
                };
                let ancestor = std::iter::successors(Some(name), |&node| tree.parent(node)).any(|node| node == owner);
                if !ancestor || !out.lookup_scopes(lookup).any(|scope| out.scopes[scope as usize].node == owner) {
                    out.failure(item, "outside lookup must select a local ancestor opening scopes".into());
                    continue;
                }
                lookup.outside = Some(owner);
                out.asked.insert(name, lookup);
            }
            let start = tree.node(name).start;
            let key = match expression {
                Some(expression) => match out.evaluate_key(item, name, expression, operand) {
                    Some(key) => Cow::Owned(key.into()),
                    None => continue,
                },
                None if out.declared.contains_key(&name) => name_key(name),
                None => match out.keys.get(&name) {
                    Some(Some(key)) => Cow::Owned(key.to_string()),
                    Some(None) => continue,
                    None => name_key(name),
                },
            };
            let found = out.lookup_scopes(lookup).find_map(|scope| {
                scopes[scope as usize].declared.get(&key).and_then(|decls| visible(decls, start, want))
            });
            if let Some(found) = found.filter(|&found| found != name) {
                out.declaration.insert(name, found);
                out.references.entry(found).or_default().push(name);
            }
        }
        for refs in out.references.values_mut() {
            refs.sort_by_key(|&r| tree.node(r).start);
        }
        out.scope_of = scope_of;
        out.complete = out.failures.is_empty();
        out
    }
}

/// Path steps that are not fields: the node's parent (`parent.parameter`),
/// the node something unwraps from (`it`), the reference a node was reached
/// through (`through`), and a name's declaration.
pub(crate) const PARENT: u16 = u16::MAX;
pub const IT: u16 = u16::MAX - 1;
pub const THROUGH: u16 = u16::MAX - 2;
pub const DECLARATION: u16 = u16::MAX - 3;
/// The node's index among its parent's nodes in the same field (a type
/// parameter's position, for `from through.type_argument at position`).
pub const POSITION: u16 = u16::MAX - 4;
pub const RESULT: u16 = u16::MAX - 5;
pub const EXPECTED: u16 = u16::MAX - 6;
pub const INFERRED: u16 = u16::MAX - 7;
pub const RETURNED: u16 = u16::MAX - 8;
pub const WIDENED: u16 = u16::MAX - 9;
pub const RECEIVER: u16 = u16::MAX - 10;
pub const PRODUCT_ITEMS: u16 = u16::MAX - 11;
pub const FIRST_PARAMETER: u16 = u16::MAX - 12;
/// The lowest special step: attribute steps stay below it.
const SPECIAL_STEPS: u16 = FIRST_PARAMETER;
/// `ATTR_STEP | id`: the node a `let` attribute (by engine attribute id)
/// holds; fields are numbered below it, the special steps above.
pub const ATTR_STEP: u16 = 0x8000;

/// Steps that ask a value question (`result`, `expected`, `inferred`, ...)
/// instead of leading to syntax sites.
pub fn query_step(step: u16) -> bool {
    matches!(step, RESULT | EXPECTED | INFERRED | RETURNED | WIDENED | RECEIVER | FIRST_PARAMETER)
}

pub fn attribute_step(step: u16) -> Option<u16> {
    (ATTR_STEP..SPECIAL_STEPS).contains(&step).then_some(step & !ATTR_STEP)
}

/// The bits of `classes`, adding new ones to `list` (at most 64).
fn class_bits(list: &mut Vec<String>, classes: &[Name], errors: &mut Vec<ConfigError>) -> Option<u64> {
    let mut bits = 0;
    for c in classes {
        let i = match list.iter().position(|x| *x == c.text) {
            Some(i) => i,
            None if list.len() < 64 => {
                list.push(c.text.clone());
                list.len() - 1
            }
            None => {
                errors.push(ConfigError::new(c.span, "at most 64 classes of one kind"));
                return None;
            }
        };
        bits |= 1 << i;
    }
    Some(bits)
}

/// The declaration a use at `start` sees among one scope's declarations of
/// its name: the latest one before it that is not hoisted (shadowing), else
/// the first hoisted one matching the requested class.
/// Implicit declarations count only when nothing declares the name here.
fn visible(decls: &[Declared], start: u32, want: u64) -> Option<u32> {
    let matching = || decls.iter().filter(|d| d.classes & want != 0 && !d.implicit);
    let shadowing = matching().rfind(|d| !d.hoisted && d.start <= start);
    let hoisted = || matching().find(|d| d.hoisted);
    shadowing.or_else(hoisted).or_else(|| decls.iter().find(|d| d.implicit && d.classes & want != 0)).map(|d| d.name)
}

impl Resolved {
    /// The declaration of `classes` that `node` (a use or a declaration)
    /// stands for: among merged declarations (`interface X` + `namespace
    /// X`), the first that declares one of them, else the first.
    pub fn declaration_in(&self, node: u32, classes: u64) -> u32 {
        let declares = |name: u32| self.declared.get(&name).is_some_and(|&(_, _, declared, _)| declared & classes != 0);
        if declares(node) { return node }
        let first = self.declaration.get(&node).copied().unwrap_or(node);
        if declares(first) { return first }
        self.merged_later.get(&first).and_then(|later| later.iter().copied().find(|&name| declares(name))).unwrap_or(first)
    }

    fn failure(&mut self, node: u32, message: Box<str>) {
        if !self.failures.iter().any(|(at, previous)| *at == node && *previous == message) {
            self.failures.push((node, message));
        }
    }

    fn evaluate_filter(&mut self, node: u32, target: u32, expression: u32, operand: &ResolveOperand<'_>) -> bool {
        match operand(node, expression, Some(target)) {
            Ok(crate::expr::Value::Bool(value)) => value,
            Ok(_) => {
                self.failure(node, "declaration filter must return a boolean".into());
                false
            }
            Err(message) => {
                self.failure(node, message);
                false
            }
        }
    }

    fn evaluate_key(&mut self, node: u32, target: u32, expression: u32, operand: &ResolveOperand<'_>) -> Option<Box<str>> {
        let key = match operand(node, expression, Some(target)) {
            Ok(crate::expr::Value::Str(text)) => Some(Box::<str>::from(&*text)),
            Ok(_) => None,
            Err(message) => {
                self.failure(node, message);
                None
            }
        };
        self.keys.entry(target).or_insert(key).clone()
    }

    /// The innermost scope `node` sits in (its own when it opens one),
    /// respecting `within`. Names are syntax-only, so this holds in every
    /// generic frame.
    pub fn scope_of(&self, node: u32) -> Option<u32> {
        self.scope_of.get(node as usize).copied().filter(|&s| s != NO_SCOPE)
    }

    pub fn lookup_scopes(&self, lookup: ReferenceLookup) -> impl Iterator<Item = u32> + '_ {
        std::iter::successors(Some(lookup.scope), |&scope| self.scopes.get(scope as usize)?.parent)
            .filter(move |&scope| self.scopes.get(scope as usize).is_some_and(|scope| Some(scope.node) != lookup.outside))
    }

    /// The tree's declarations by scope and name. `key` must be the file's
    /// name key: the index is built once, with the first caller's.
    pub fn index(&self, tree: &Tree, text: &str, key: &NameKey<'_>) -> &ScopeIndex {
        self.index.get_or_init(|| {
            let mut index = ScopeIndex { declared: vec![FxHashMap::default(); self.scopes.len()], opened: FxHashMap::default() };
            for (i, scope) in self.scopes.iter().enumerate() {
                let i = i as u32;
                index.opened.entry(scope.node).or_insert((i, i)).1 = i + 1;
            }
            let memberships = self.declared.iter().map(|(&name, &(scope, ..))| (name, scope))
                .chain(self.also_declared.iter().flat_map(|(&name, others)| others.iter().map(move |&(scope, ..)| (name, scope))));
            for (name, scope) in memberships {
                let Some(names) = index.declared.get_mut(scope as usize) else { continue };
                match self.implicit.get(&name) {
                    Some(texts) => texts.iter().for_each(|t| names.entry(t.clone()).or_default().push(name)),
                    None => {
                        let n = tree.node(name);
                        let named = match self.keys.get(&name) {
                            Some(Some(key)) => key.clone(),
                            Some(None) => continue,
                            None => key(&text[n.start as usize..n.end as usize]).into(),
                        };
                        names.entry(named).or_default().push(name);
                    }
                }
            }
            for names in &mut index.declared {
                names.values_mut().for_each(|decls| decls.sort_by_key(|&d| (tree.node(d).start, d)));
            }
            index
        })
    }

    /// How `name` is declared in `scope` (`declared` or one of
    /// `also_declared`); the primary membership when it is in neither.
    pub fn membership(&self, name: u32, scope: u32) -> (u32, u32, u64, bool) {
        let primary = self.declared.get(&name).copied().unwrap_or((scope, name, 0, false));
        if primary.0 == scope { return primary }
        self.also_declared.get(&name).and_then(|others| others.iter().find(|other| other.0 == scope)).copied().unwrap_or(primary)
    }

    /// The declaration `name` (a name's key) means at `at`, searching the
    /// scopes around it outward for one of `classes`.
    pub fn visible(&self, tree: &Tree, text: &str, key: &NameKey<'_>, at: u32, name: &str, classes: u64) -> Option<u32> {
        self.visible_except(tree, text, key, at, name, classes, &[])
    }

    /// [`Self::visible`] as if the `skipped` declarations did not exist
    /// (an import proven not to carry the class gives way to the next).
    #[allow(clippy::too_many_arguments)]
    pub fn visible_except(&self, tree: &Tree, text: &str, key: &NameKey<'_>, at: u32, name: &str, classes: u64, skipped: &[u32]) -> Option<u32> {
        if !self.failures.is_empty() { return None }
        let index = self.index(tree, text, key);
        let start = tree.node(at).start;
        let lookup = match self.asked.get(&at) {
            Some(&lookup) => lookup,
            None => {
                let scope = std::iter::successors(Some(at), |&node| tree.parent(node))
                    .find_map(|node| index.opened(node).last())
                    .or((!self.scopes.is_empty()).then_some(0))?;
                ReferenceLookup { scope, classes, outside: None }
            }
        };
        for s in self.lookup_scopes(lookup) {
            let decls: Vec<Declared> = index
                .declared(s, name)
                .iter()
                .filter(|d| !skipped.contains(d))
                .map(|&d| {
                    let (_, _, classes, hoisted) = self.membership(d, s);
                    Declared { name: d, start: tree.node(d).start, classes, hoisted, implicit: self.implicit.contains_key(&d) }
                })
                .collect();
            if let Some(found) = visible(&decls, start, classes) {
                return Some(found);
            }
        }
        None
    }
}

#[cfg(test)]
mod keyed_tests;
#[cfg(test)]
mod filter_tests;
#[cfg(test)]
mod lookup_tests;

#[cfg(test)]
mod tests {
    use super::{visible, Declared};

    const VALUE: u64 = 1;
    const TYPE: u64 = 2;

    #[test]
    fn target_depth_exhaustion_is_unknown_not_a_terminal() {
        use super::{question::Known, KindNames, Names};
        use crate::grammar::{Edge, Node, Tree};
        let tables = Names {
            kinds: vec![KindNames { targets: vec![(vec![vec![0]], None)], ..KindNames::default() }],
            ..Names::default()
        };
        let chain = |length: u32| Tree::from_native((0..length).map(|at| Node {
            matcher: if at + 1 == length { 1 } else { 0 }, start: 0, end: 0,
            children: if at + 1 == length { vec![] } else { vec![Edge { label: 0, node: at + 1 }] },
        }).collect(), 0);
        assert_eq!(tables.targets_of(&chain(2), 0, &|_, _| Known::Yes(true)), Known::Yes(vec![1]));
        assert_eq!(tables.targets_of(&chain(300), 0, &|_, _| Known::Yes(true)), Known::Unknown);
    }

    #[test]
    fn target_cycles_are_unknown_not_terminal_nodes() {
        use super::{question::Known, KindNames, Names, IT};
        use crate::grammar::{Node, Tree};
        let tables = Names {
            kinds: vec![KindNames { targets: vec![(vec![vec![IT]], None)], ..KindNames::default() }],
            ..Names::default()
        };
        let tree = Tree::from_native(vec![Node { matcher: 0, start: 0, end: 0, children: vec![] }], 0);
        assert_eq!(tables.targets_of(&tree, 0, &|_, _| Known::Yes(true)), Known::Unknown);
    }

    fn d(name: u32, start: u32, classes: u64, hoisted: bool) -> Declared {
        Declared { name, start, classes, hoisted, implicit: false }
    }

    #[test]
    fn hoisted_lookups_select_the_requested_declaration_class() {
        let decls = [d(1, 10, TYPE, true), d(2, 20, VALUE | TYPE, true), d(3, 30, VALUE, false)];
        assert_eq!(visible(&decls, 25, VALUE), Some(2));
        assert_eq!(visible(&decls, 25, TYPE), Some(1));
        assert_eq!(visible(&decls, 40, VALUE), Some(3));
    }

    #[test]
    fn later_declarations_shadow_and_hoisted_ones_merge_into_the_first() {
        // `let x` at 10 and 30: a use at 20 sees the first, at 40 the second.
        let lets = [d(1, 10, VALUE, false), d(2, 30, VALUE, false)];
        assert_eq!(visible(&lets, 5, VALUE), None);
        assert_eq!(visible(&lets, 20, VALUE), Some(1));
        assert_eq!(visible(&lets, 40, VALUE), Some(2));
        // `var x` twice: one binding, the first, visible before both.
        let vars = [d(3, 10, VALUE, true), d(4, 30, VALUE, true)];
        assert_eq!(visible(&vars, 5, VALUE), Some(3));
        assert_eq!(visible(&vars, 40, VALUE), Some(3));
        assert_eq!(visible(&[d(5, 10, TYPE, true)], 30, VALUE), None);
        let both = [d(5, 10, TYPE, true), d(6, 20, VALUE, true)];
        assert_eq!(visible(&both, 30, VALUE), Some(6));
        assert_eq!(visible(&both, 30, TYPE), Some(5));
    }

    #[test]
    fn implicit_declarations_yield_to_explicit_ones() {
        // `function f(arguments)`: the parameter, not the implicit binding.
        let implicit = Declared { name: 1, start: 0, classes: VALUE, hoisted: true, implicit: true };
        assert_eq!(visible(&[implicit, d(2, 11, VALUE, true)], 20, VALUE), Some(2));
        assert_eq!(visible(&[implicit], 20, VALUE), Some(1));
    }
}
