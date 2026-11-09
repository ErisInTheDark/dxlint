use dx_config::ast::{Config, Name, NameItem};
use dx_config::error::ConfigError;
use dx_grammar::Grammar;

use crate::values::compile::lower_value;
use crate::{
    attribute_step, calls, effects, modules, question, values, Condition, Declare, Effect, KindNames, Names, OpenScope,
    Operand, Resolve, Unwrapping, Value, Vocabulary, ATTR_STEP, DECLARATION, EXPECTED, FIRST_PARAMETER, INFERRED, IT,
    PARENT, POSITION, RECEIVER, RESULT, RETURNED, SPECIAL_STEPS, THROUGH, WIDENED,
};

impl Vocabulary {
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
pub(crate) fn intern(list: &mut Vec<String>, name: &str) -> usize {
    list.iter().position(|x| x == name).unwrap_or_else(|| {
        list.push(name.to_string());
        list.len() - 1
    })
}

/// Resolves a field path to labels, reporting unknown fields.
pub(crate) type PathOf<'a> = dyn Fn(&mut Vec<ConfigError>, &[Name]) -> Option<Vec<u16>> + 'a;

pub(crate) fn lower_path(g: &Grammar, attrs: &dx_ir::expr::AttrNames, errors: &mut Vec<ConfigError>, path: &[Name]) -> Option<Vec<u16>> {
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

/// The bits of `classes`, adding new ones to `list` (at most 64).
pub(crate) fn class_bits(list: &mut Vec<String>, classes: &[Name], errors: &mut Vec<ConfigError>) -> Option<u64> {
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

impl Names {
    /// The tables of `config`'s name items. Classes come from the items
    /// naming them; one looked up or searched for but never declared or
    /// opened is an error, like an unknown matcher.
    pub fn build(config: &Config, g: &Grammar, errors: &mut Vec<ConfigError>,
        constant: &dx_grammar::Constant) -> Names {
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
        let attrs = dx_ir::expr::AttrNames::new(config.attrs.iter().map(|d| d.name.text.as_str()));
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
                        dx_config::ast::values::IndexedMode::Slots { repeated, strings } => values::IndexedMode::Slots { repeated: *repeated, strings: *strings },
                        dx_config::ast::values::IndexedMode::Domain(domain) => values::IndexedMode::Domain(*domain),
                        dx_config::ast::values::IndexedMode::Keyed(key) => {
                            let Some(key) = lower_value(key, &path, &mut vocab, errors, &mut requested_classes) else { continue };
                            values::IndexedMode::Keyed(key)
                        }
                        dx_config::ast::values::IndexedMode::Keys { bind } => {
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
                    let scope = matches!(module, dx_config::ast::modules::Item::Scope { .. });
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
                        Some(dx_config::ast::modules::Asking::All) => u64::MAX,
                        Some(dx_config::ast::modules::Asking::Classes(classes)) => {
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
        names.module_resolutions = modules::compile::profiles(config, g, &vocab, errors, &mut |definition, mode| names.mode_operand(definition, mode), constant);
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
        self.operands.push(Operand { of: dx_config::ast::OperandOf::Item(item), matcher, part, expr: std::sync::OnceLock::new(), lowered: std::sync::OnceLock::new() });
        self.operands.len() as u32 - 1
    }

    pub fn mode_operand(&mut self, definition: usize, mode: usize) -> u32 {
        let id = self.operands.len() as u32;
        self.operands.push(Operand { of: dx_config::ast::OperandOf::Mode { definition, mode }, matcher: dx_tree::ROOT, part: 0, expr: std::sync::OnceLock::new(), lowered: std::sync::OnceLock::new() });
        id
    }

    /// Applies `f` to `m` and every matcher specialising it (`like`).
    fn add(&mut self, g: &Grammar, m: u32, f: impl Fn(&mut KindNames)) {
        for &i in g.specialisations(m) {
            if let Some(kind) = self.kinds.get_mut(i as usize) { f(kind) }
        }
    }
}
