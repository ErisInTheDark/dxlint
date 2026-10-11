//! Native reads go through the engine's recorded door
//! ([`crate::engine::reads`]); this adds TypeScript's global declaration
//! file discovery to it.

use std::path::{Path, PathBuf};

pub use crate::engine::reads::*;

/// The TypeScript files below a directory that add to the global scope.
pub static GLOBAL_FILES: Discovery = Discovery { name: "ts-globals", wanted: typescript, matches: declares_globals };

fn typescript(path: &Path) -> bool {
    let name = path.to_string_lossy();
    name.ends_with(".ts") || name.ends_with(".tsx")
}

/// Cheap negative screen: a global augmentation's next token is `{`.
fn global_augmentation_candidate(text: &str) -> bool {
    text.match_indices("global").any(|(offset, _)| {
        let rest = text[offset + "global".len()..].trim_start_matches(|c: char| c.is_ascii_whitespace() || super::lexer::js_space(c));
        // Comments remain candidates; only the parser decides what they mean.
        matches!(rest.chars().next(), Some('{' | '/'))
    })
}

/// Does a TypeScript file add to the global scope (a declaration script,
/// or a `declare global`)?
fn declares_globals(path: &Path, text: &str) -> bool {
    let name = path.to_string_lossy();
    let declaration = name.ends_with(".d.ts");
    if declaration {
        if !text.contains("import") && !text.contains("export") {
            return true;
        }
    } else if !global_augmentation_candidate(text) {
        return false;
    }
    let Ok(parsed) = super::parse(text, name.ends_with(".tsx")) else { return false };
    let view = super::view::Raw { src: text, nodes: &parsed.nodes };
    use super::view::View;
    declaration && !view.all(parsed.root, super::kinds::Field::Body).into_iter().any(|node| super::view::external_module_statement(&view, node))
        || super::view::global_augmentation(&view, parsed.root)
}

/// The TypeScript files below `dir` (gitignore respected) that add to the
/// global scope, in lexical path order, including unsaved editor buffers.
pub fn global_files(dir: &Path, jobs: usize) -> Vec<PathBuf> {
    discover(&GLOBAL_FILES, dir, jobs)
}

/// The walked directories whose [`global_files`] the `changed` files change.
pub fn globals_changed(changed: &[PathBuf]) -> Vec<PathBuf> {
    discovery_changed(&GLOBAL_FILES, changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_declaration_detection_uses_syntax_not_quoted_words() {
        let declaration = Path::new("globals.d.ts");
        for source in [
            r#"// export ignored
declare const value: "import marker";"#,
            "/* import fake */ declare const value: number;",
            "declare module 'ambient' { export const value: number; }",
            "import Alias = Local.Member; declare namespace Local { class Member {} }",
        ] {
            assert!(declares_globals(declaration, source), "{source}");
        }
        for source in [
            "export\n{}; declare const value: number;",
            "import\n{ Value } from 'module';",
            "import Alias = require('module');",
        ] {
            assert!(!declares_globals(declaration, source), "{source}");
        }
        let implementation = Path::new("module.ts");
        assert!(!declares_globals(implementation, r#"const text = "declare global";"#));
        assert!(!declares_globals(implementation, "// declare global\nexport {};"));
        for source in [
            "export {}; declare\n global { interface Window { value: number } }",
            "export {}; declare global /* marker */ { interface Window { value: number } }",
            "export {}; declare global // marker\n{ interface Window { value: number } }",
            "export {}; declare\u{feff}global\u{2007}{ interface Window { value: number } }",
        ] {
            assert!(declares_globals(implementation, source), "{source}");
        }
    }

    #[test]
    fn global_module_precedence_orders_disk_and_buffer_files_together() {
        use super::super::modules::{ModRef, Project};
        let dir = std::env::current_dir().unwrap().join(format!("target/global-precedence-{}", std::process::id()));
        let config = dir.join("tsconfig.json");
        let first = dir.join("a.d.ts");
        let last = dir.join("z.d.ts");
        set_buffer(&config, r#"{"compilerOptions":{"lib":[],"types":[]}}"#);
        set_buffer(&first, r#"declare module "order" { const winner: "first"; }"#);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&last, r#"declare module "order" { const winner: "last"; }"#).unwrap();
        let resolved = [1, 4].map(|jobs| {
            let mut project = Project::default();
            project.set_discovery_jobs(jobs);
            let from = project.file_id(&dir.join("main.ts"));
            match project.resolve(from, "order") {
                Some(ModRef::Ambient(file, _)) => Some(project.path(file)),
                _ => None,
            }
        });
        std::fs::remove_file(last).unwrap();
        std::fs::remove_dir(&dir).unwrap();
        drop_buffer(&config);
        drop_buffer(&first);
        assert_eq!(resolved, [Some(first.clone()), Some(first)]);
    }

    #[test]
    fn global_discovery_modes_preserve_membership_reads_and_buffers() {
        let dir = std::env::current_dir().unwrap().join(format!("target/global-discovery-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        for (name, text) in [
            (".gitignore", "ignored.d.ts\n"),
            ("z.d.ts", "declare const Last: number;"),
            ("a.ts", "export {};"),
            ("ignored.d.ts", "declare const Ignored: number;"),
        ] { std::fs::write(dir.join(name), text).unwrap(); }
        let first = dir.join("a.ts");
        let virtual_file = dir.join("virtual/global.tsx");
        set_buffer(&first, "export {}; declare global { interface First {} }");
        set_buffer(&virtual_file, "export {}; declare global { interface Virtual {} }");
        let (serial, recorded) = track(|| global_files(&dir, 1));
        let parallel = global_files(&dir, 4);
        let same = recorded.deps.iter().all(|dep| dep.holds(|_| 0));
        set_buffer(&first, "export {};");
        let changed = recorded.deps.iter().any(|dep| !dep.holds(|_| 0));
        drop_buffer(&first);
        drop_buffer(&virtual_file);
        for name in [".gitignore", "z.d.ts", "a.ts", "ignored.d.ts"] { std::fs::remove_file(dir.join(name)).unwrap(); }
        std::fs::remove_dir(dir.join(".git")).unwrap();
        std::fs::remove_dir(&dir).unwrap();
        assert_eq!(serial, parallel);
        assert_eq!(serial, [first, virtual_file, dir.join("z.d.ts")]);
        assert!(same, "parallel discovery must hash membership like serial validation");
        assert!(changed, "a buffer ceasing to declare globals must invalidate membership");
    }

    #[test]
    fn buffered_global_declarations_are_discovered() {
        let dir = std::env::temp_dir().join(format!("zlint-buffer-globals-{}", std::process::id()));
        let file = dir.join("virtual/nested/globals.d.ts");
        set_buffer(&file, "declare const PackedGlobal: number;");
        let (found, reads) = track(|| global_files(&dir, 1));
        assert_eq!(found.as_slice(), std::slice::from_ref(&file));
        assert!(reads.deps.iter().all(|dep| dep.holds(|_| 0)));
        drop_buffer(&file);
        assert!(reads.deps.iter().any(|dep| !dep.holds(|_| 0)));
    }
}
