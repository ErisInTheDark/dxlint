//! Regex compilation shared by config schemas and rule expressions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub use regex_automata::meta::Regex;

fn shared() -> &'static Mutex<HashMap<String, Arc<Regex>>> {
    static SHARED: OnceLock<Mutex<HashMap<String, Arc<Regex>>>> = OnceLock::new();
    SHARED.get_or_init(Default::default)
}

/// The compiled pattern, or a readable error.
pub fn compile(pattern: &str) -> Result<Arc<Regex>, String> {
    if let Some(re) = shared().lock().ok().and_then(|c| c.get(pattern).cloned()) {
        return Ok(re);
    }
    let re = Arc::new(Regex::new(pattern).map_err(|e| format!("invalid regex `{pattern}`: {e}"))?);
    if let Ok(mut c) = shared().lock() {
        c.insert(pattern.to_string(), re.clone());
    }
    Ok(re)
}
