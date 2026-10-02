use serde::Serialize;

use crate::config::ast::Severity;

/// Replace `start..end` (byte offsets) with `text`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub start: usize,
    pub end: usize,
    /// All edits apply together or not at all.
    pub fix: Option<Vec<Edit>>,
}

/// Rule name for problems the engine itself reports.
pub const ENGINE_RULE: &str = "zlint";
