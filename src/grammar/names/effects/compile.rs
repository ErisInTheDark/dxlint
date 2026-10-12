use super::{After, Catch, Conditional, Flow, KindEffects, Path};
use crate::config::ast::Name;
use crate::config::ast::effects::{Flow as ParsedFlow, Item as ParsedItem};
use crate::config::error::ConfigError;
use crate::config::source::Span;
use crate::fx_hash::FxHashSet;
use crate::grammar::names::Names;

#[derive(Clone, Debug)]
pub enum Item {
    Runs(Flow),
    Entry(Path),
    Catches(Catch),
    After(After),
    Labels(Path),
}

pub fn effect_roles(vocabulary: &crate::grammar::names::Vocabulary) -> Vec<super::EffectRole> {
    vocabulary.effects.iter().map(|name| {
        match super::Control::named(name) {
            Some(control) => super::EffectRole::Control(control),
            None if name == "truthy" => super::EffectRole::Proof,
            None => super::EffectRole::Event,
        }
    }).collect()
}

pub fn lower(
    item: &ParsedItem,
    path: &mut impl FnMut(&[Name]) -> Option<Path>,
    effect: &mut impl FnMut(&Name) -> Option<u16>,
) -> Option<Item> {
    Some(match item {
        ParsedItem::Runs(value) => Item::Runs(flow(value, path)?),
        ParsedItem::Entry(value) => Item::Entry(path(value)?),
        ParsedItem::Catches { handler, effects, source } => Item::Catches(Catch {
            handler: path(handler)?,
            source: path(source)?,
            effects: effects.iter().map(effect).collect::<Option<_>>()?,
        }),
        ParsedItem::After { finalizer, sources } => Item::After(After {
            finalizer: path(finalizer)?,
            sources: sources.iter().map(|source| path(source)).collect::<Option<_>>()?,
        }),
        ParsedItem::Labels(value) => Item::Labels(path(value)?),
    })
}

fn flow(value: &ParsedFlow, path: &mut impl FnMut(&[Name]) -> Option<Path>) -> Option<Flow> {
    Some(match value {
        ParsedFlow::Nothing => Flow::Nothing,
        ParsedFlow::Field(value) => Flow::Field(path(value)?),
        ParsedFlow::Then(parts) => Flow::Then(parts.iter().map(|part| flow(part, path)).collect::<Option<_>>()?),
        ParsedFlow::Or(parts) => Flow::Or(parts.iter().map(|part| flow(part, path)).collect::<Option<_>>()?),
        ParsedFlow::Maybe(value) => Flow::Maybe(Box::new(flow(value, path)?)),
        ParsedFlow::Repeat { body, test, next, first } => {
            let mut test = match test { Some(test) => Some(path(test)?), None => None };
            let truth = test.as_ref().and_then(|path| path.last()).copied()
                .and_then(crate::grammar::names::attribute_step);
            if truth.is_some() { test.as_mut()?.pop(); }
            Flow::Repeat {
                body: Box::new(flow(body, path)?),
                test,
                truth,
                next: match next { Some(next) => Some(Box::new(flow(next, path)?)), None => None },
                first: *first,
            }
        },
        ParsedFlow::Select { entries, test, body } => Flow::Select { entries: path(entries)?, test: path(test)?, body: path(body)? },
    })
}

impl KindEffects {
    pub fn push(&mut self, item: Item, when: Option<u32>, item_id: u32, span: Span) {
        match item {
            Item::Runs(item) => self.runs.push(Conditional { item, when, item_id, span }),
            Item::Entry(item) => self.entries.push(Conditional { item, when, item_id, span }),
            Item::Catches(item) => self.catches.push(Conditional { item, when, item_id, span }),
            Item::After(item) => self.after.push(Conditional { item, when, item_id, span }),
            Item::Labels(item) => self.labels.push(Conditional { item, when, item_id, span }),
        }
    }
}

pub fn validate(names: &Names, errors: &mut Vec<ConfigError>) {
    let mut reported = FxHashSet::default();
    let mut report = |item: u32, span: Span, message: &'static str| {
        if reported.insert((item, message)) {
            errors.push(ConfigError::new(span, message));
        }
    };
    for named in &names.kinds {
        let terminators: Vec<_> = named.unwraps.iter().filter(|item| item.when.is_none() && !item.possible
            && item.effect.and_then(|id| names.effect_roles.get(id as usize)).copied().and_then(super::EffectRole::control).is_some()).collect();
        for item in terminators.iter().skip(1) {
            if item.item != terminators[0].item {
                if let Some(span) = item.span {
                    report(item.item, span, "more than one unconditional terminating effect");
                }
            }
        }
        let kind = &named.execution;
        let runs: Vec<_> = kind.runs.iter().filter(|item| item.when.is_none()).collect();
        for run in runs.iter().skip(1) {
            if run.item_id != runs[0].item_id {
                report(run.item_id, run.span, "more than one unconditional execution shape");
            }
        }
        let catches: Vec<_> = kind.catches.iter().filter(|item| item.when.is_none()).collect();
        let after: Vec<_> = kind.after.iter().filter(|item| item.when.is_none()).collect();
        for finalizer in after.iter().skip(1) {
            if finalizer.item_id != after[0].item_id {
                report(finalizer.item_id, finalizer.span, "more than one unconditional finalizer");
            }
        }
        for (index, catch) in catches.iter().enumerate() {
            if catches[..index].iter().any(|previous| previous.item_id != catch.item_id
                && previous.item.source == catch.item.source
                && previous.item.effects.iter().any(|effect| catch.item.effects.contains(effect))) {
                report(catch.item_id, catch.span, "handlers for this source catch the same effect");
            }
        }
        let Some(run) = runs.first() else { continue };
        let Some(mut sources) = run.item.sources() else { continue };
        sources.extend(catches.iter().map(|catch| catch.item.handler.clone()));
        for after in after {
            if sources.iter().any(|source| !after.item.sources.contains(source)) {
                report(after.item_id, after.span, "`runs after` must cover the whole execution and its handlers");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::grammar::Grammar;

    fn valid(items: &str) -> bool {
        let source = format!(
            "lang demo \"*.demo\"\nin .demo match unit /a/\nin .demo match owner /</ block:unit handler:unit finalizer:unit />/\nin owner {{\n{items}\n}}\n"
        );
        let loaded = crate::config::load_text("effects.zlint", &source, &crate::config::source::Disk);
        let (config, mut errors) = (loaded.config, loaded.errors);
        assert!(errors.is_empty(), "execution declarations must parse: {errors:?}");
        Grammar::build(&config, &mut errors);
        errors.is_empty()
    }

    #[test]
    #[ignore = "RED until `assigned .. with` parses"]
    fn assignments_accept_shared_value_recipes_and_plain_targets() {
        assert!(valid("assigned block with handler ?? finalizer"));
        assert!(valid("assigned block"));
    }

    #[test]
    fn conflicting_unconditional_flows_are_configuration_errors() {
        assert!(!valid("runs block\nruns handler"));
        assert!(valid("when true { runs block }\nwhen false { runs handler }"));
    }

    #[test]
    fn overlapping_handlers_for_one_source_are_configuration_errors() {
        assert!(!valid("runs block\nin handler catches thrown from block\nin finalizer catches thrown from block"));
        assert!(valid("runs block\nin handler catches thrown from block\nin finalizer catches returned from block"));
    }

    #[test]
    fn finalizers_must_cover_execution_and_its_handlers() {
        assert!(!valid("runs block\nin handler catches thrown from block\nin finalizer runs after block"));
        assert!(!valid("runs block then handler\nin finalizer runs after block"));
        assert!(valid("runs block\nin handler catches thrown from block\nin finalizer runs after block, handler"));
    }

    #[test]
    fn conflicting_unconditional_finalizers_are_configuration_errors() {
        assert!(!valid("runs block\nin handler runs after block\nin finalizer runs after block"));
        assert!(valid("runs block\nwhen true { in handler runs after block }\nwhen false { in finalizer runs after block }"));
    }

    #[test]
    fn conflicting_terminating_effects_are_configuration_errors() {
        assert!(valid("runs block\nunwraps returned block"));
        assert!(!valid("runs block\nunwraps returned block\nunwraps thrown block"));
        assert!(valid("runs block\nunwraps returned block\nunwraps maybe thrown block"));
    }
}
