use super::{After, Catch, Conditional, Flow, KindEffects, Path};
use crate::config::ast::Name;
use crate::config::ast::effects::{Flow as ParsedFlow, Item as ParsedItem};

#[derive(Clone, Debug)]
pub enum Item {
    Runs(Flow),
    Entry(Path),
    Catches(Catch),
    After(After),
    Labels(Path),
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
        ParsedFlow::Repeat { body, test, next, first } => Flow::Repeat {
            body: Box::new(flow(body, path)?),
            test: match test { Some(test) => Some(path(test)?), None => None },
            next: match next { Some(next) => Some(Box::new(flow(next, path)?)), None => None },
            first: *first,
        },
        ParsedFlow::Select { entries, test, body } => Flow::Select { entries: path(entries)?, test: path(test)?, body: path(body)? },
    })
}

impl KindEffects {
    pub fn push(&mut self, item: Item, when: Option<u32>) {
        match item {
            Item::Runs(item) => self.runs.push(Conditional { item, when }),
            Item::Entry(item) => self.entries.push(Conditional { item, when }),
            Item::Catches(item) => self.catches.push(Conditional { item, when }),
            Item::After(item) => self.after.push(Conditional { item, when }),
            Item::Labels(item) => self.labels.push(Conditional { item, when }),
        }
    }
}
