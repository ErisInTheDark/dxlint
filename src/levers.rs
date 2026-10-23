//! Experimental engine levers, switched per process by
//! `ZLINT_LEVERS=<name>,<name>` (`all` turns every one on), so one build
//! can measure many changes against each other. A lever lives here only
//! until it is measured: kept levers lose their switch, rejected ones their
//! code. Off by default; with none set output is the released behaviour.

use std::sync::OnceLock;

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Lever {
    /// No lever yet: keeps the mechanism compiling between experiments.
    Placeholder,
}

const NAMES: &[(&str, Lever)] = &[("placeholder", Lever::Placeholder)];

/// Whether `lever` is switched on for this process.
pub fn on(lever: Lever) -> bool {
    static SET: OnceLock<u64> = OnceLock::new();
    let set = *SET.get_or_init(|| {
        let Some(list) = std::env::var_os("ZLINT_LEVERS") else { return 0 };
        list.to_string_lossy().split(',').map(str::trim).fold(0, |set, name| match name {
            "all" => u64::MAX,
            name => NAMES.iter().find(|(known, _)| *known == name).map_or(set, |&(_, lever)| set | 1 << lever as u8),
        })
    });
    set & 1 << lever as u8 != 0
}
