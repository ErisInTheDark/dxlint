use super::Name;

pub type Path = Vec<Name>;

#[derive(Clone, Debug)]
pub enum Flow {
    Nothing,
    Field(Path),
    Then(Vec<Flow>),
    Or(Vec<Flow>),
    Maybe(Box<Flow>),
    Repeat {
        body: Box<Flow>,
        test: Option<Path>,
        next: Option<Box<Flow>>,
        first: bool,
    },
    Select { entries: Path, test: Path, body: Path },
}

#[derive(Clone, Debug)]
pub enum Item {
    Runs(Flow),
    Entry(Path),
    Catches { handler: Path, effects: Vec<Name>, source: Path },
    After { finalizer: Path, sources: Vec<Path> },
    Labels(Path),
}
