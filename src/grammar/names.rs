//! Names a language declares with `scope`, `declares` and `resolves` items
//! (docs/design/type-layers.md): which nodes open scopes, which declare
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
}

/// One `resolves` item.
#[derive(Clone, Debug)]
pub struct Resolve {
    /// Declaration classes (bits) looked up.
    pub classes: u64,
    /// Labels from the node to the using name; empty: the node itself.
    pub path: Vec<u16>,
    pub when: Option<u32>,
}

/// An `unwraps` / `propagates` value with fields as labels
/// ([`crate::config::ast::Unwrap`]).
#[derive(Clone, Debug)]
pub enum Value {
    Path(Vec<u16>),
    Text(Box<str>),
    Either(Box<Value>, Box<Value>),
    Union(Vec<Value>),
    Intersection(Vec<Value>),
    Spread { intersection: bool, path: Vec<u16> },
    /// The member `key` of what the first value unwraps to, among scopes of
    /// these classes (0: the leaf kind's [`KindNames::member_defaults`]).
    From(Box<Value>, Box<Value>, u64),
    /// The scopes of these classes the nodes reached open.
    Scope { classes: u64, from: Box<Value> },
    Not(Box<Value>),
    /// Trait bits ([`Vocabulary::traits`]), and of what (default: the node).
    Excludes { traits: u64, from: Option<Box<Value>> },
    Includes { value: Box<Value>, from: Option<Box<Value>> },
    Has { key: Box<Value>, from: Option<Box<Value>> },
    /// A positional key: a type argument or tuple slot.
    Ordinal(u32),
    /// A global declaration of these classes, instantiated with these
    /// arguments by position.
    Instance { name: Box<str>, classes: u64, arguments: Vec<Value> },
}

/// One `unwraps` item: what the node stands for (no effect), or what
/// evaluating it does.
#[derive(Clone, Debug)]
pub struct Unwrapping {
    /// Index into [`Vocabulary::effects`].
    pub effect: Option<u16>,
    /// `maybe`: the effect may happen, or the node completes normally.
    pub possible: bool,
    pub value: Option<Value>,
    /// Scope classes of `nearest` (`returned nearest function`).
    pub nearest: u64,
    pub when: Option<u32>,
}

/// One effect on a field path, for the effects names already answer
/// (writes: `assigned`, `mutated`, user effects).
#[derive(Clone, Debug)]
pub struct Effect {
    /// Index into [`Names::effects`].
    pub effect: u16,
    pub path: Vec<u16>,
    pub when: Option<u32>,
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
    /// Traits (bits) the node directly has, each under its condition.
    pub fits: Vec<(u64, Option<u32>)>,
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
    /// `members .. inherits`: scope classes and where inherited members
    /// come from, searched after the kind's own.
    pub member_bases: Vec<(u64, Value, Option<u32>)>,
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
}

/// An expression one name item evaluates on its layer's nodes (a module
/// specifier, a literal's identity): compiled with that layer's fields and
/// lowered to the VM like a [`Condition`]; read through
/// [`question::World::operand`].
pub struct Operand {
    /// Index of the item in the config's name items.
    pub item: usize,
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

/// An `unwraps` value with its field paths as labels.
fn lower_value(v: &crate::config::ast::Unwrap, path: &PathOf<'_>, vocab: &mut Vocabulary, errors: &mut Vec<ConfigError>) -> Option<Value> {
    use crate::config::ast::Unwrap as U;
    let lower = |v: &U, vocab: &mut Vocabulary, errors: &mut Vec<ConfigError>| lower_value(v, path, vocab, errors);
    let traits = vocab;
    Some(match v {
        U::Path(p) => Value::Path(path(errors, p)?),
        U::Text(t) => Value::Text(t.as_str().into()),
        U::Either(a, b) => Value::Either(Box::new(lower(a, traits, errors)?), Box::new(lower(b, traits, errors)?)),
        U::Union(parts) => Value::Union(parts.iter().map(|p| lower(p, traits, errors)).collect::<Option<_>>()?),
        U::Intersection(parts) => Value::Intersection(parts.iter().map(|p| lower(p, traits, errors)).collect::<Option<_>>()?),
        U::Spread { intersection, path: p } => Value::Spread { intersection: *intersection, path: path(errors, p)? },
        U::From(of, key, classes) => {
            let bits = class_bits(&mut traits.scope_classes, classes, errors)?;
            Value::From(Box::new(lower(of, traits, errors)?), Box::new(lower(key, traits, errors)?), bits)
        }
        U::Scope { classes, from } => {
            let bits = class_bits(&mut traits.scope_classes, classes, errors)?;
            Value::Scope { classes: bits, from: Box::new(lower(from, traits, errors)?) }
        }
        U::Not(a) => Value::Not(Box::new(lower(a, traits, errors)?)),
        U::Excludes { traits: names, from } => {
            let mut bits = 0u64;
            for t in names {
                let i = intern(&mut traits.traits, &t.text);
                if i >= 64 {
                    errors.push(ConfigError::new(t.span, "at most 64 traits"));
                    return None;
                }
                bits |= 1 << i;
            }
            let from = match from {
                Some(f) => Some(Box::new(lower(f, traits, errors)?)),
                None => None,
            };
            Value::Excludes { traits: bits, from }
        }
        U::Includes { value, from } | U::Has { key: value, from } => {
            let value = Box::new(lower(value, traits, errors)?);
            let from = match from {
                Some(f) => Some(Box::new(lower(f, traits, errors)?)),
                None => None,
            };
            if matches!(v, U::Includes { .. }) { Value::Includes { value, from } } else { Value::Has { key: value, from } }
        }
        U::Ordinal(n) => Value::Ordinal(*n),
        U::Instance { name, class, arguments } => {
            let classes = class_bits(&mut traits.declaration_classes, std::slice::from_ref(class), errors)?;
            let arguments = arguments.iter().map(|a| lower(a, traits, errors)).collect::<Option<_>>()?;
            Value::Instance { name: name.text.as_str().into(), classes, arguments }
        }
    })
}

/// The tables, per matcher (empty when the config has no name items).
#[derive(Default)]
pub struct Names {
    pub vocabulary: std::sync::Arc<Vocabulary>,
    pub kinds: Vec<KindNames>,
    pub conditions: Vec<Condition>,
    /// Every item expression evaluated on nodes ([`Operand`]).
    pub operands: Vec<Operand>,
    /// Module item operands, compiled like [`Names::conditions`].
    pub modules: modules::compile::Tables,
    /// `exclusive` groups: trait bits no possible value has together.
    pub exclusive: Vec<u64>,
}

/// One tree's resolved names.
#[derive(Default)]
pub struct Resolved {
    /// What the class bits and effect indexes below mean.
    pub vocabulary: std::sync::Arc<Vocabulary>,
    /// Using name -> the name node that declares it (the first of merged
    /// declarations); a later merged declaration -> the first.
    pub declaration: FxHashMap<u32, u32>,
    /// Later declarations of a merged name (`var a; var a;`, `class X` +
    /// `interface X`) -> the first, which owns the binding.
    pub merged: FxHashMap<u32, u32>,
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
    /// Implicit declarations (`declares value "arguments"`): node -> names.
    pub implicit: FxHashMap<u32, Vec<Box<str>>>,
    /// Using name -> the scope it is looked up from and the classes asked.
    pub asked: FxHashMap<u32, (u32, u64)>,
    /// Declarations by scope and name, built on first ask ([`Resolved::index`]).
    index: std::sync::OnceLock<ScopeIndex>,
}

/// What a name's text means as a key (a language's escapes):
/// [`crate::engine::frontend::Frontend::name_key`].
pub type NameKey<'k> = dyn for<'a> Fn(&'a str) -> Cow<'a, str> + 'k;

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
        if config.names.is_empty() && config.exclusive.is_empty() {
            return names;
        }
        let mut vocab = Vocabulary::default();
        // Numbered as the engine numbers them (first definition order).
        let attrs = crate::expr::AttrNames::new(config.attrs.iter().map(|d| d.name.text.as_str()));
        let mut declared = 0u64;
        let mut opened = 0u64;
        let mut asked: Vec<(&Name, bool)> = Vec::new();
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
                // Fields win over the special steps, those over `let` attributes.
                path.iter()
                    .map(|f| match (g.labels.get(&f.text), f.text.as_str()) {
                        (Some(label), _) if label < ATTR_STEP => Ok(label),
                        (Some(_), _) => {
                            errors.push(ConfigError::new(f.span, "too many fields for name item paths"));
                            Err(())
                        }
                        (None, "parent") => Ok(PARENT),
                        (None, "it") => Ok(IT),
                        (None, "through") => Ok(THROUGH),
                        (None, "definition") => Ok(DECLARATION),
                        (None, "position") => Ok(POSITION),
                        (None, name) => match attrs.get(name) {
                            Some(attr) if attr < POSITION - ATTR_STEP => Ok(ATTR_STEP | attr),
                            _ => {
                                errors.push(
                                    ConfigError::new(f.span, format!("no layer has a field or attribute `{}`", f.text))
                                        .note("path steps are fields, `let` attributes, or `parent`, `it`, `through`, `definition`, `position`"),
                                );
                                Err(())
                            }
                        },
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .ok()
            };
            // One condition per target layer: it reads that layer's fields.
            let when: Vec<Option<u32>> = targets
                .iter()
                .map(|&matcher| {
                    def.when.as_ref().map(|_| {
                        names.conditions.push(Condition { item, matcher, expr: std::sync::OnceLock::new(), lowered: std::sync::OnceLock::new() });
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
                NameItem::Declares { classes, path: p, implicit, nearest, enclosing, hoisted } => {
                    let Some(bits) = class_bits(&mut vocab.declaration_classes, classes, errors) else { continue };
                    let Some(near) = class_bits(&mut vocab.scope_classes, nearest, errors) else { continue };
                    declared |= bits;
                    asked.extend(nearest.iter().map(|n| (n, true)));
                    let Some(p) = path(errors, p) else { continue };
                    let implicit: Option<Box<str>> = implicit.as_deref().map(Into::into);
                    for (&m, &when) in targets.iter().zip(&when) {
                        let d = Declare { classes: bits, path: p.clone(), nearest: near, hoisted: *hoisted, enclosing: *enclosing, implicit: implicit.clone(), when };
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
                NameItem::Unwraps { effect, possible, value, nearest } => {
                    let effect = effect.as_ref().map(|e| intern(&mut vocab.effects, &e.text) as u16);
                    let Some(near) = class_bits(&mut vocab.scope_classes, nearest, errors) else { continue };
                    asked.extend(nearest.iter().map(|n| (n, true)));
                    let value = match value {
                        Some(v) => match lower_value(v, &path, &mut vocab, errors) {
                            Some(v) => Some(v),
                            None => continue,
                        },
                        None => None,
                    };
                    for (&m, &when) in targets.iter().zip(&when) {
                        // An effect on a field: the targets it reaches carry it (writes).
                        if let (Some(effect), Some(Value::Path(p))) = (effect, &value) {
                            let e = Effect { effect, path: p.clone(), when };
                            names.add(g, m, |k| k.effects.push(e.clone()));
                        }
                        let u = Unwrapping { effect, possible: *possible, value: value.clone(), nearest: near, when };
                        names.add(g, m, |k| k.unwraps.push(u.clone()));
                    }
                }
                NameItem::Execution(item) => {
                    let Some(item) = effects::compile::lower(item, &mut |p| path(errors, p), &mut |e| u16::try_from(intern(&mut vocab.effects, &e.text)).ok()) else {
                        continue;
                    };
                    for (&m, &when) in targets.iter().zip(&when) {
                        names.add(g, m, |k| k.execution.push(item.clone(), when));
                    }
                }
                NameItem::Members { classes, inherits } => {
                    let Some(bits) = class_bits(&mut vocab.scope_classes, classes, errors) else { continue };
                    asked.extend(classes.iter().map(|n| (n, true)));
                    let inherits = match inherits {
                        Some(v) => match lower_value(v, &path, &mut vocab, errors) {
                            Some(v) => Some(v),
                            None => continue,
                        },
                        None => None,
                    };
                    for (&m, &when) in targets.iter().zip(&when) {
                        names.add(g, m, |k| k.member_defaults |= bits);
                        if let Some(base) = &inherits {
                            names.add(g, m, |k| k.member_bases.push((bits, base.clone(), when)));
                        }
                    }
                }
                NameItem::Member { classes, key, value } => {
                    let Some(bits) = class_bits(&mut vocab.scope_classes, classes, errors) else { continue };
                    asked.extend(classes.iter().map(|n| (n, true)));
                    let Some(key) = lower_value(key, &path, &mut vocab, errors) else { continue };
                    let Some(value) = lower_value(value, &path, &mut vocab, errors) else { continue };
                    for (&m, &when) in targets.iter().zip(&when) {
                        let link = (bits, key.clone(), value.clone(), when);
                        names.add(g, m, |k| k.member_links.push(link.clone()));
                    }
                }
                NameItem::Fits(traits) => {
                    let Some(bits) = class_bits(&mut vocab.traits, traits, errors) else { continue };
                    for (&m, &when) in targets.iter().zip(&when) {
                        names.add(g, m, |k| k.fits.push((bits, when)));
                    }
                }
                NameItem::Propagates { field, value } => {
                    let Some(field) = path(errors, field) else { continue };
                    let Some(value) = lower_value(value, &path, &mut vocab, errors) else { continue };
                    for (&m, &when) in targets.iter().zip(&when) {
                        let p = (field.clone(), value.clone(), when);
                        names.add(g, m, |k| k.propagates.push(p.clone()));
                    }
                }
                NameItem::Resolves { classes, path: p } => {
                    let Some(bits) = class_bits(&mut vocab.declaration_classes, classes, errors) else { continue };
                    asked.extend(classes.iter().map(|c| (c, false)));
                    let Some(p) = path(errors, p) else { continue };
                    for (&m, &when) in targets.iter().zip(&when) {
                        let r = Resolve { classes: bits, path: p.clone(), when };
                        names.add(g, m, |k| k.resolves.push(r.clone()));
                    }
                }
                NameItem::Generics(p) => {
                    let Some(p) = path(errors, p) else { continue };
                    for (&m, &when) in targets.iter().zip(&when) {
                        names.add(g, m, |k| k.generics.push((p.clone(), when)));
                    }
                }
                NameItem::Calls(call) => {
                    for (&m, &when) in targets.iter().zip(&when) {
                        let lowered = calls::compile::lower(
                            call,
                            &mut |p, errors| path(errors, p),
                            &mut |v, errors| lower_value(v, &path, &mut vocab, errors),
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
                    for (&m, &when) in targets.iter().zip(&when) {
                        if scope {
                            let open = OpenScope { classes: bits, within: Vec::new(), when };
                            names.add(g, m, |k| k.scopes.push(open.clone()));
                        }
                        let lowered = names.modules.lower(module, item, m, bits, when);
                        names.add(g, m, |k| k.modules.items.push(lowered.clone()));
                    }
                }
            }
        }
        for (name, scope) in asked {
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
        names.exclusive = values::exclusive_groups(config, &mut vocab, &names.kinds, errors);
        names.vocabulary = std::sync::Arc::new(vocab);
        names
    }

    /// Registers expression `part` of name item `item` on `matcher`'s nodes.
    pub fn operand(&mut self, item: usize, matcher: u32, part: u8) -> u32 {
        self.operands.push(Operand { item, matcher, part, expr: std::sync::OnceLock::new(), lowered: std::sync::OnceLock::new() });
        self.operands.len() as u32 - 1
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
    fn targets(&self, tree: &Tree, node: u32, holds: &dyn Fn(u32, Option<u32>) -> bool, out: &mut Vec<u32>, passed: &mut Vec<u32>, depth: u32) {
        let targets = self.kinds.get(tree.node(node).matcher as usize).map_or(&[][..], |k| &k.targets[..]);
        let mut targets = targets.iter().filter(|(_, when)| holds(node, *when)).peekable();
        if targets.peek().is_none() || depth > 256 {
            out.push(node);
            return;
        }
        let (reached, passing) = (out.len(), passed.len());
        passed.push(node);
        // Per group, the first alternative that reaches anything: `value ??
        // key` falls back to the key when the value is a pattern reaching
        // nothing (`{ a = 1 }`).
        for (group, _) in targets {
            for path in group {
                let before = out.len();
                for n in follow(tree, node, path) {
                    self.targets(tree, n, holds, out, passed, depth + 1);
                }
                if out.len() > before {
                    break;
                }
            }
        }
        if out.len() == reached {
            passed.truncate(passing);
        }
    }

    /// Resolves every name `tree` uses. `key` turns a name's text into what
    /// it means (a language's escapes); `when(node, condition)` evaluates a
    /// `when` condition on a node.
    pub fn resolve<'t>(&self, tree: &Tree, text: &'t str, key: &dyn Fn(&'t str) -> Cow<'t, str>, when: &dyn Fn(u32, u32) -> bool) -> Resolved {
        let mut out = Resolved { vocabulary: self.vocabulary.clone(), ..Resolved::default() };
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
        let mut uses: Vec<(u32, usize, u64)> = Vec::new();
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
                    let entry = Declared { name: id, start: tree.node(id).start, classes: d.classes, hoisted: true, implicit: true };
                    scopes[target].declared.entry(Cow::Owned(name.to_string())).or_default().push(entry);
                    out.implicit.entry(id).or_default().push(name.clone());
                    out.declared.insert(id, (target as u32, id, d.classes, true));
                    continue;
                }
                declared.clear();
                for named in follow(tree, id, &d.path) {
                    self.targets(tree, named, &holds, &mut declared, &mut passed, 0);
                }
                passed.clear();
                for &name in &declared {
                    let entry = Declared { name, start: tree.node(name).start, classes: d.classes, hoisted: d.hoisted, implicit: false };
                    scopes[target].declared.entry(name_key(name)).or_default().push(entry);
                    out.declared.entry(name).or_insert((target as u32, id, d.classes, d.hoisted));
                }
            }
            for e in kind.effects.iter().filter(|e| holds(id, e.when)) {
                declared.clear();
                passed.clear();
                for target in follow(tree, id, &e.path) {
                    self.targets(tree, target, &holds, &mut declared, &mut passed, 0);
                }
                for &node in passed.iter().chain(&declared) {
                    out.effects.entry(node).or_insert((e.effect, id));
                }
            }
            if let Some(here) = here {
                for r in kind.resolves.iter().filter(|r| holds(id, r.when)) {
                    if r.path.is_empty() {
                        uses.push((id, here, r.classes));
                        continue;
                    }
                    // A use aimed at a qualified name or parentheses passes
                    // on through `targets` (`A.B` in a type looks up `A`).
                    declared.clear();
                    for named in follow(tree, id, &r.path) {
                        self.targets(tree, named, &holds, &mut declared, &mut passed, 0);
                    }
                    passed.clear();
                    uses.extend(declared.iter().map(|&name| (name, here, r.classes)));
                }
            }
        }
        // A later hoisted declaration of a merged name stands for the first.
        for scope in &scopes {
            for decls in scope.declared.values() {
                let mut hoisted = decls.iter().filter(|d| d.hoisted && !d.implicit);
                if let Some(first) = hoisted.next() {
                    for later in hoisted.filter(|d| d.name != first.name) {
                        out.declaration.insert(later.name, first.name);
                        out.merged.insert(later.name, first.name);
                    }
                }
            }
        }
        // A name looked up twice keeps the first lookup, found or not: a
        // parent's `resolves type name` comes before the name's own
        // `resolves value`.
        // Declaring names are declarations, not uses.
        for (name, scope, want) in uses {
            if out.asked.contains_key(&name) || out.declared.contains_key(&name) {
                continue;
            }
            out.asked.insert(name, (scope as u32, want));
            let start = tree.node(name).start;
            let key = name_key(name);
            let mut at = Some(scope);
            while let Some(s) = at {
                let decls = scopes[s].declared.get(&key);
                if let Some(found) = decls.and_then(|decls| visible(decls, start, want)) {
                    if found != name {
                        out.declaration.insert(name, found);
                        out.references.entry(found).or_default().push(name);
                    }
                    break;
                }
                at = out.scopes[s].parent.map(|p| p as usize);
            }
        }
        for refs in out.references.values_mut() {
            refs.sort_by_key(|&r| tree.node(r).start);
        }
        out
    }
}

/// Path steps that are not fields: the node's parent (`parent.parameter`),
/// the node something unwraps from (`it`), the reference a node was reached
/// through (`through`), and a name's declaration.
const PARENT: u16 = u16::MAX;
pub const IT: u16 = u16::MAX - 1;
pub const THROUGH: u16 = u16::MAX - 2;
pub const DECLARATION: u16 = u16::MAX - 3;
/// The node's index among its parent's nodes in the same field (a type
/// parameter's position, for `from through.type_argument at position`).
pub const POSITION: u16 = u16::MAX - 4;
/// `ATTR_STEP | id`: the node a `let` attribute (by engine attribute id)
/// holds; fields are numbered below it, the special steps above.
pub const ATTR_STEP: u16 = 0x8000;

/// The nodes a structural `path` (field labels, [`PARENT`], [`IT`]) leads to
/// from `from` in one tree. Semantic steps ([`THROUGH`], [`DECLARATION`],
/// [`POSITION`], attributes) lead nowhere here: the values engine owns them.
pub fn follow(tree: &Tree, from: u32, path: &[u16]) -> Vec<u32> {
    let mut nodes = vec![from];
    for &label in path {
        nodes = match label {
            PARENT => nodes.into_iter().filter_map(|n| tree.parent(n)).collect(),
            IT => nodes,
            // Unwrapping steps: not names' business.
            THROUGH | DECLARATION | POSITION => Vec::new(),
            _ if label >= ATTR_STEP => Vec::new(),
            _ => nodes.into_iter().flat_map(|n| tree.fields(n, label)).collect(),
        };
    }
    nodes
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
/// the first hoisted one; merged hoisted declarations (of any class:
/// `interface X` + `namespace X`, overloads) are one binding, the first.
/// Implicit declarations count only when nothing declares the name here.
fn visible(decls: &[Declared], start: u32, want: u64) -> Option<u32> {
    let matching = || decls.iter().filter(|d| d.classes & want != 0 && !d.implicit);
    let shadowing = matching().rfind(|d| !d.hoisted && d.start <= start);
    let hoisted = || matching().any(|d| d.hoisted).then(|| decls.iter().find(|d| d.hoisted && !d.implicit)).flatten();
    shadowing.or_else(hoisted).or_else(|| decls.iter().find(|d| d.implicit && d.classes & want != 0)).map(|d| d.name)
}

impl Resolved {
    /// The tree's declarations by scope and name. `key` must be the file's
    /// name key: the index is built once, with the first caller's.
    pub fn index(&self, tree: &Tree, text: &str, key: &NameKey<'_>) -> &ScopeIndex {
        self.index.get_or_init(|| {
            let mut index = ScopeIndex { declared: vec![FxHashMap::default(); self.scopes.len()], opened: FxHashMap::default() };
            for (i, scope) in self.scopes.iter().enumerate() {
                let i = i as u32;
                index.opened.entry(scope.node).or_insert((i, i)).1 = i + 1;
            }
            for (&name, &(scope, ..)) in &self.declared {
                let Some(names) = index.declared.get_mut(scope as usize) else { continue };
                match self.implicit.get(&name) {
                    Some(texts) => texts.iter().for_each(|t| names.entry(t.clone()).or_default().push(name)),
                    None => {
                        let n = tree.node(name);
                        names.entry(key(&text[n.start as usize..n.end as usize]).into()).or_default().push(name);
                    }
                }
            }
            for names in &mut index.declared {
                names.values_mut().for_each(|decls| decls.sort_by_key(|&d| (tree.node(d).start, d)));
            }
            index
        })
    }

    /// The declaration `name` (a name's key) means at `at`, searching the
    /// scopes around it outward for one of `classes`.
    pub fn visible(&self, tree: &Tree, text: &str, key: &NameKey<'_>, at: u32, name: &str, classes: u64) -> Option<u32> {
        let index = self.index(tree, text, key);
        let start = tree.node(at).start;
        let mut node = Some(at);
        let mut scope = None;
        while let (None, Some(n)) = (scope, node) {
            scope = index.opened(n).last();
            node = tree.parent(n);
        }
        let mut scope = scope.or((!self.scopes.is_empty()).then_some(0));
        while let Some(s) = scope {
            let decls: Vec<Declared> = index
                .declared(s, name)
                .iter()
                .map(|&d| {
                    let (_, _, classes, hoisted) = self.declared[&d];
                    Declared { name: d, start: tree.node(d).start, classes, hoisted, implicit: self.implicit.contains_key(&d) }
                })
                .collect();
            if let Some(found) = visible(&decls, start, classes) {
                return Some(found);
            }
            scope = self.scopes[s as usize].parent;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{visible, Declared};

    const VALUE: u64 = 1;
    const TYPE: u64 = 2;

    fn d(name: u32, start: u32, classes: u64, hoisted: bool) -> Declared {
        Declared { name, start, classes, hoisted, implicit: false }
    }

    #[test]
    fn hoisted_declarations_of_any_class_merge_into_the_first() {
        // `type X` then `namespace X`: a value lookup finds the namespace,
        // which merges into the type alias; a later `let X` shadows both.
        let decls = [d(1, 10, TYPE, true), d(2, 20, VALUE | TYPE, true), d(3, 30, VALUE, false)];
        assert_eq!(visible(&decls, 25, VALUE), Some(1));
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
        // Namespaces: a type declaration alone is invisible to value
        // lookups; beside a value of the name, both are one binding.
        assert_eq!(visible(&[d(5, 10, TYPE, true)], 30, VALUE), None);
        let both = [d(5, 10, TYPE, true), d(6, 20, VALUE, true)];
        assert_eq!(visible(&both, 30, VALUE), Some(5));
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
