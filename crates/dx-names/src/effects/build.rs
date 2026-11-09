//! How an effect ends a step: the controls `runs` flows name. Building the
//! flows themselves lives in `dx-semantic`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Returned,
    /// Leaves its target like `returned` but carries no returned value
    /// (an iterator closed from outside).
    Exited,
    Thrown,
    Broken,
    Continued,
}

impl Control {
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "returned" => Some(Self::Returned),
            "exited" => Some(Self::Exited),
            "thrown" => Some(Self::Thrown),
            "broken" => Some(Self::Broken),
            "continued" => Some(Self::Continued),
            _ => None,
        }
    }
}
