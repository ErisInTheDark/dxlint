//! Embeds the built-in library: every `std/**/*.zlint` and `rules/*.zlint`,
//! so adding a module is adding its file (`src/config/std_lib.rs` reads the
//! generated lists).

use std::fmt::Write as _;
use std::path::Path;

fn modules(dir: &Path, rel: &str, out: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).expect("library dir").map(|e| e.expect("entry").path()).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().expect("name").to_string_lossy().into_owned();
        let rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if path.is_dir() {
            modules(&path, &rel, out);
        } else if let Some(module) = rel.strip_suffix(".zlint") {
            out.push(module.to_string());
        }
    }
}

fn list(root: &Path, dir: &str, name: &str, code: &mut String) {
    println!("cargo:rerun-if-changed={dir}");
    let mut found = Vec::new();
    modules(&root.join(dir), "", &mut found);
    writeln!(code, "static {name}: &[(&str, &str)] = &[").expect("write");
    for module in found {
        let path = root.join(dir).join(format!("{module}.zlint"));
        writeln!(code, "    ({module:?}, include_str!({:?})),", path.to_string_lossy().replace('\\', "/")).expect("write");
    }
    writeln!(code, "];").expect("write");
}

fn main() {
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let mut code = String::new();
    list(&root, "std", "FILES", &mut code);
    list(&root, "rules", "RULESETS", &mut code);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("out dir"));
    std::fs::write(out.join("library.rs"), code).expect("write library list");
}
