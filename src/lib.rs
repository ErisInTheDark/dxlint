//! dxlint: an extremely fast linter whose rules are written in the
//! `.dx` language.
//!
//! - [`config`] reads `.dx` files (macros, imports, parsing)
//! - [`pattern`] is the regex subset matchers are written in
//! - [`grammar`] turns matchers into layers and parses files into trees
//! - [`expr`] evaluates formatter expressions
//! - [`engine`] runs formatters over trees and applies fixes
//! - `vm` evaluates attributes and rule conditions lowered to typed code

pub mod alloc;
pub mod cli;
pub mod config;
pub mod engine;
pub mod expr;
pub mod footprint;
pub mod fx_hash;
pub mod grammar;
pub mod levers;
pub mod line_index;
pub mod lsp;
pub mod native;
pub mod path_globs;
pub mod pattern;
pub mod sampler;
pub(crate) mod js_regex;
pub(crate) mod vm;

/// The native front ends this build has (`native ts`).
pub(crate) static FRONTENDS: &[&dyn engine::frontend::Frontend] = &[&native::ts::frontend::TS];

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
