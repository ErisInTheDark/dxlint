//! What `protocol` methods (served by `dxlint map --protocol`) ask the
//! process serving them: a file's mapping and its config's identity. The
//! server installs a [`Host`] for its lifetime; elsewhere these functions
//! fail.

use std::cell::RefCell;

use super::Value;

pub trait Host {
    /// `text` of `path` as its config's transforms map it: `{ text, pieces,
    /// problems }` ([`crate::engine::transform::Mapped`]).
    fn map_file(&mut self, path: &str, text: &str) -> Result<Value<'static>, String>;
    /// What a mapping depends on besides file text, for the config `path`
    /// (a file or directory) falls under.
    fn config_identity(&mut self, path: &str) -> Result<Value<'static>, String>;
    /// The config files a change to which changes mappings under `path`.
    fn config_files(&mut self, path: &str) -> Result<Value<'static>, String>;
}

thread_local! {
    static HOST: RefCell<Option<Box<dyn Host>>> = const { RefCell::new(None) };
}

/// Serves protocol requests on this thread with `host` until dropped.
pub fn install(host: Box<dyn Host>) -> Installed {
    HOST.with(|slot| *slot.borrow_mut() = Some(host));
    Installed
}

pub struct Installed;

impl Drop for Installed {
    fn drop(&mut self) {
        HOST.with(|slot| *slot.borrow_mut() = None);
    }
}

pub(super) fn with<'s>(name: &str, ask: impl FnOnce(&mut dyn Host) -> Result<Value<'static>, String>) -> Result<Value<'s>, String> {
    HOST.with(|slot| match slot.borrow_mut().as_deref_mut() {
        Some(host) => ask(host),
        None => Err(format!("`{name}` answers only inside a `protocol` method served by `dxlint map --protocol`")),
    })
}
