//! dxlint: an extremely fast linter whose rules are written in the
//! `.dx` language.
//!
//! - [`config`] reads `.dx` files (macros, imports, parsing)
//! - [`pattern`] is the regex subset matchers are written in
//! - [`grammar`] turns matchers into layers and parses files into trees
//! - [`expr`] evaluates formatter expressions
//! - [`engine`] runs formatters over trees and applies fixes
//! - `vm` evaluates attributes and rule conditions lowered to typed code

pub mod cli;
pub use dx_config as config;
pub mod engine;
pub mod expr;
pub use dx_grammar as grammar;
#[cfg(test)]
mod grammar_tests;
pub mod lsp;
pub use dx_core::{alloc, fx_hash, levers, parts, path_globs, reads, sampler};
pub use dx_tree::{footprint, line_index};
pub use dx_pattern as pattern;
#[cfg(feature = "work")]
pub use dx_core::work;
pub(crate) mod vm;

/// The test binary allocates like the shipped one (`main.rs`): `npm run
/// check` runs fixtures and self-lint from it.
#[cfg(test)]
#[global_allocator]
static TEST_ALLOCATOR: alloc::Pooled = alloc::Pooled;

/// A crash ends the process with its exit code instead of a modal error
/// dialog: dxlint runs unattended (CI, editors, batch runs, test runs), where
/// a dialog blocks until someone clicks it. Child processes inherit the mode.
#[cfg(windows)]
pub fn no_error_dialogs() {
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;
    const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
    extern "system" {
        fn SetErrorMode(mode: u32) -> u32;
    }
    // SAFETY: SetErrorMode only sets this process's error mode.
    unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX); }
}

#[cfg(not(windows))]
pub fn no_error_dialogs() {}

/// Test binaries have no `main` of ours: the C runtime calls this before the
/// test harness starts.
#[cfg(all(test, windows))]
#[used]
#[unsafe(link_section = ".CRT$XCU")]
static TEST_NO_ERROR_DIALOGS: extern "C" fn() = {
    extern "C" fn set() { no_error_dialogs() }
    set
};

#[cfg(all(test, windows))]
#[test]
fn test_processes_never_show_crash_dialogs() {
    extern "system" {
        fn GetErrorMode() -> u32;
    }
    // SAFETY: reads this process's error mode.
    assert_ne!(unsafe { GetErrorMode() } & 0x0002, 0, "a crashing test would block on a dialog");
}
