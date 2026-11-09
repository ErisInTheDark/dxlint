//! Effects: the execution shapes `runs`, `catches`, `runs after` and `labels`
//! are declared with. Building, walking and executing them lives in
//! `dx-semantic`.

pub mod build;
pub mod compile;

pub use build::Control;

pub type Path = Vec<u16>;
pub type StepId = u32;
pub type EffectId = u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectRole {
    Control(Control),
    Proof,
    Read,
    Event { writes: bool },
}

impl EffectRole {
    pub fn control(self) -> Option<Control> {
        match self {
            Self::Control(control) => Some(control),
            Self::Proof | Self::Read | Self::Event { .. } => None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Flow {
    Children,
    Nothing,
    Field(Path),
    Then(Vec<Flow>),
    Or(Vec<Flow>),
    Maybe(Box<Flow>),
    Repeat {
        body: Box<Flow>,
        test: Option<Path>,
        truth: Option<u16>,
        next: Option<Box<Flow>>,
        first: bool,
    },
    Select { entries: Path, test: Path, body: Path },
}

impl Flow {
    pub fn sources(&self) -> Option<dx_core::fx_hash::FxHashSet<Path>> {
        let mut sources = dx_core::fx_hash::FxHashSet::default();
        let mut pending = vec![self];
        while let Some(flow) = pending.pop() {
            match flow {
                Self::Children => return None,
                Self::Nothing => {}
                Self::Field(path) => { sources.insert(path.clone()); }
                Self::Then(parts) | Self::Or(parts) => pending.extend(parts),
                Self::Maybe(flow) => pending.push(flow),
                Self::Repeat { body, test, next, .. } => {
                    pending.push(body);
                    if let Some(test) = test { sources.insert(test.clone()); }
                    if let Some(next) = next { pending.push(next); }
                }
                Self::Select { entries, .. } => { sources.insert(entries.clone()); }
            }
        }
        Some(sources)
    }
}

#[derive(Clone, Debug)]
pub struct Conditional<T> {
    pub item: T,
    pub when: Option<u32>,
    pub item_id: u32,
    pub span: dx_config::source::Span,
}

#[derive(Clone, Debug)]
pub struct Catch {
    pub handler: Path,
    pub source: Path,
    pub effects: Vec<EffectId>,
}

#[derive(Clone, Debug)]
pub struct After {
    pub finalizer: Path,
    pub sources: Vec<Path>,
}

#[derive(Clone, Debug, Default)]
pub struct KindEffects {
    pub runs: Vec<Conditional<Flow>>,
    pub entries: Vec<Conditional<Path>>,
    pub catches: Vec<Conditional<Catch>>,
    pub after: Vec<Conditional<After>>,
    pub labels: Vec<Conditional<Path>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Enter,
    Leave,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Read,
    Write,
    Effect(EffectId),
}
