use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::{DeclRef, FileId, ModRef, ModuleAt, Project, MAX_HOPS};
use crate::native::ts::summary::{DeclKind, Export, Summary};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SymbolSpaces {
    pub type_space: bool,
    pub value: bool,
    pub namespace: bool,
}

impl SymbolSpaces {
    fn merge(&mut self, other: Self) {
        self.type_space |= other.type_space;
        self.value |= other.value;
        self.namespace |= other.namespace;
    }

    fn type_only(mut self, type_only: bool) -> Self {
        if type_only { self.value = false; }
        self
    }
}

pub struct ModuleSymbols {
    pub exports: BTreeMap<Box<str>, Option<SymbolSpaces>>,
    pub complete: bool,
}

impl Project {
    pub fn declaration_spaces(&self, declarations: &[DeclRef]) -> Option<SymbolSpaces> {
        self.spaces_of(declarations, MAX_HOPS)
    }

    fn spaces_of(&self, declarations: &[DeclRef], hops: usize) -> Option<SymbolSpaces> {
        if declarations.is_empty() || hops == 0 { return None }
        let mut spaces = SymbolSpaces::default();
        for &reference in declarations {
            let summary = self.summary(reference.file)?;
            let declaration = &summary.decls[reference.decl as usize];
            let next = match &declaration.kind {
                DeclKind::Interface | DeclKind::Alias => SymbolSpaces { type_space: true, ..Default::default() },
                DeclKind::Class | DeclKind::Enum => SymbolSpaces { type_space: true, value: true, namespace: declaration.kind == DeclKind::Enum },
                DeclKind::Var | DeclKind::Function => SymbolSpaces { value: true, ..Default::default() },
                DeclKind::Namespace => SymbolSpaces { value: declaration.namespace_value, namespace: true, ..Default::default() },
                DeclKind::Import { from, name } => {
                    let module = self.resolve(reference.file, from)?;
                    if &**name == "*" {
                        self.module_of(module)?;
                        SymbolSpaces { value: true, namespace: true, ..Default::default() }
                    } else {
                        self.export_spaces_inner(module, name, hops - 1, &mut HashSet::new())?
                    }
                }
                DeclKind::ImportAlias(path) => {
                    let local = self.local_path(reference.file, &summary, declaration.module, declaration.parent, path, hops - 1)?;
                    self.spaces_of(&local, hops - 1)?
                }
                DeclKind::Lexical => return None,
            };
            spaces.merge(next.type_only(declaration.type_only));
        }
        Some(spaces)
    }

    fn local_path(&self, file: FileId, summary: &Summary, module: Option<usize>, parent: Option<u32>, path: &[Box<str>], hops: usize) -> Option<Vec<DeclRef>> {
        if hops == 0 { return None }
        let (root, members) = path.split_first()?;
        let mut enclosing = parent;
        let mut ids = Vec::new();
        while let Some(index) = enclosing {
            let declaration = &summary.decls[index as usize];
            if let Some(scope) = &declaration.scope { ids.extend_from_slice(scope.get(root)); }
            if !ids.is_empty() { break }
            enclosing = declaration.parent;
        }
        if ids.is_empty() {
            let scope = module.map_or(&summary.top, |index| &summary.ambient_modules[index].2);
            ids.extend_from_slice(scope.get(root));
        }
        let mut declarations = ids.into_iter().map(|decl| DeclRef { file, decl }).collect::<Vec<_>>();
        if declarations.is_empty() { declarations = self.globals(file).names.get(root).cloned().unwrap_or_default(); }
        for member in members {
            let mut next = Vec::new();
            for reference in declarations {
                let summary = self.summary(reference.file)?;
                let declaration = &summary.decls[reference.decl as usize];
                match &declaration.kind {
                    DeclKind::Namespace => {
                        if let Some(scope) = &declaration.scope {
                            next.extend(scope.get(member).iter().map(|&decl| DeclRef { file: reference.file, decl }));
                        }
                    }
                    DeclKind::Import { from, name } if &**name == "*" => {
                        next.extend(self.export(self.resolve(reference.file, from)?, member));
                    }
                    DeclKind::ImportAlias(path) => {
                        let mut path = path.to_vec();
                        path.push(member.clone());
                        next.extend(self.local_path(reference.file, &summary, declaration.module, declaration.parent, &path, hops - 1)?);
                    }
                    _ => return None,
                }
            }
            declarations = next;
        }
        (!declarations.is_empty()).then_some(declarations)
    }

    pub fn export_spaces(&self, module: ModRef, name: &str) -> Option<SymbolSpaces> {
        self.export_spaces_inner(module, name, MAX_HOPS, &mut HashSet::new())
    }

    fn export_spaces_inner(&self, module_ref: ModRef, name: &str, hops: usize, seen: &mut HashSet<(ModRef, Box<str>)>) -> Option<SymbolSpaces> {
        let key = (module_ref, name.into());
        if hops == 0 || !seen.insert(key.clone()) { return None }
        let result = self.export_spaces_lookup(module_ref, name, hops, seen);
        seen.remove(&key);
        result
    }

    fn export_spaces_lookup(&self, module_ref: ModRef, name: &str, hops: usize, seen: &mut HashSet<(ModRef, Box<str>)>) -> Option<SymbolSpaces> {
        let (file, summary, at) = self.module_of(module_ref)?;
        let (module, scope) = at.get(&summary);
        if let Some(binding) = module.exports.get(name) {
            let spaces = match &binding.target {
                Export::Decls(ids) => self.scope_spaces(file, ids, hops - 1)?,
                Export::Reexport { from, name } if from.is_empty() => self.scope_spaces(file, scope.get(name), hops - 1)?,
                Export::Reexport { from, name } => self.export_spaces_inner(self.resolve(file, from)?, name, hops - 1, seen)?,
                Export::Namespace(from) => {
                    self.module_of(self.resolve(file, from)?)?;
                    SymbolSpaces { value: true, namespace: true, ..Default::default() }
                }
            };
            return Some(spaces.type_only(binding.type_only));
        }
        if name == "=" || name == "default" && module.equals.is_some() {
            let path = module.equals.as_ref()?;
            let declarations = self.local_path(file, &summary, match at { ModuleAt::Ambient(index) => Some(index), ModuleAt::Top => None }, None, path, hops - 1)?;
            return self.spaces_of(&declarations, hops - 1);
        }
        if name == "default" { return None }
        for star in &module.stars {
            let Some(target) = self.resolve(file, &star.from) else { continue };
            if let Some(spaces) = self.export_spaces_inner(target, name, hops - 1, seen) {
                return Some(spaces.type_only(star.type_only));
            }
        }
        None
    }

    fn scope_spaces(&self, file: FileId, ids: &[u32], hops: usize) -> Option<SymbolSpaces> {
        self.spaces_of(&ids.iter().map(|&decl| DeclRef { file, decl }).collect::<Vec<_>>(), hops)
    }

    pub fn module_symbols(&self, module: ModRef) -> Option<ModuleSymbols> {
        self.module_of(module)?;
        let mut names = BTreeSet::new();
        let mut complete = self.export_names(module, MAX_HOPS, &mut HashSet::new(), &mut names);
        let exports = names.into_iter().map(|name| {
            let spaces = self.export_spaces(module, &name);
            complete &= spaces.is_some();
            (name, spaces)
        }).collect();
        Some(ModuleSymbols { exports, complete })
    }

    fn export_names(&self, module_ref: ModRef, hops: usize, seen: &mut HashSet<ModRef>, names: &mut BTreeSet<Box<str>>) -> bool {
        if hops == 0 { return false }
        if !seen.insert(module_ref) { return false }
        let Some((file, summary, at)) = self.module_of(module_ref) else { return false };
        let (module, _) = at.get(&summary);
        names.extend(module.exports.keys().cloned());
        let mut complete = module.equals.is_none();
        for star in &module.stars {
            let mut inner = BTreeSet::new();
            complete &= self.resolve(file, &star.from).is_some_and(|target| self.export_names(target, hops - 1, seen, &mut inner));
            names.extend(inner.into_iter().filter(|name| &**name != "default"));
        }
        complete
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::ts::reads::{self, Dep};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Inputs {
        root: PathBuf,
        paths: Vec<PathBuf>,
    }

    impl Inputs {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            Self { root: std::env::current_dir().unwrap().join(format!("target/symbol-spaces-{}", NEXT.fetch_add(1, Ordering::Relaxed))), paths: Vec::new() }
        }

        fn set(&mut self, name: &str, source: &str) -> PathBuf {
            let path = self.root.join(name);
            reads::set_buffer(&path, source);
            self.paths.push(path.clone());
            path
        }

        fn module(&self, project: &Project, name: &str) -> ModRef {
            project.resolve(project.file_id(&self.root.join("main.ts")), name).expect("module")
        }
    }

    impl Drop for Inputs {
        fn drop(&mut self) {
            for path in &self.paths { reads::drop_buffer(path); }
        }
    }

    #[test]
    fn symbol_spaces_keep_type_only_hops_and_merged_value_declarations() {
        let mut inputs = Inputs::new();
        inputs.set("types.ts", "export type T = string; export const V = 1;");
        inputs.set("barrel.ts", "export type { V as Typed } from './types'; export * from './types';");
        inputs.set("outer.ts", "export type * from './barrel';");
        inputs.set("local.ts", "import { type V as I } from './types'; namespace N { export type X = 1; } namespace M { export const X = 1; } interface Both {} const Both = 1; export { I, N, M, Both };");
        let project = Project::default();
        let barrel = inputs.module(&project, "./barrel");
        assert!(!project.export_spaces(barrel, "Typed").unwrap().value);
        assert!(project.export_spaces(barrel, "V").unwrap().value);
        let outer = project.module_symbols(inputs.module(&project, "./outer")).unwrap();
        assert!(outer.complete);
        assert!(outer.exports.values().all(|spaces| !spaces.unwrap().value));
        let local = inputs.module(&project, "./local");
        assert!(!project.export_spaces(local, "I").unwrap().value);
        assert!(!project.export_spaces(local, "N").unwrap().value);
        assert!(project.export_spaces(local, "M").unwrap().value);
        let merged = project.export_spaces(local, "Both").unwrap();
        assert!(merged.type_space && merged.value);
    }

    #[test]
    fn unresolved_and_cyclic_exports_remain_incomplete() {
        let mut inputs = Inputs::new();
        inputs.set("a.ts", "export { T } from './b'; export * from './b';");
        inputs.set("b.ts", "export { T } from './a'; export * from './a';");
        inputs.set("missing.ts", "export * from './absent';");
        let project = Project::default();
        let cyclic = inputs.module(&project, "./a");
        assert_eq!(project.export_spaces(cyclic, "T"), None);
        assert!(!project.module_symbols(cyclic).unwrap().complete);
        assert!(!project.module_symbols(inputs.module(&project, "./missing")).unwrap().complete);
        assert!(project.resolve(project.file_id(&inputs.root.join("main.ts")), "./absent").is_none());
    }

    #[test]
    fn cached_module_summaries_replay_reads_and_changed_spaces() {
        let mut inputs = Inputs::new();
        let leaf = inputs.set("leaf.ts", "export interface T {}");
        let barrel = inputs.set("barrel.ts", "export * from './leaf';");
        let project = Project::default();
        let module = inputs.module(&project, "./barrel");
        project.module_symbols(module).unwrap();
        let (symbols, recorded) = reads::track(|| project.module_symbols(module).unwrap());
        assert!(!symbols.exports["T"].unwrap().value);
        for path in [&leaf, &barrel] {
            assert!(recorded.any(&mut |dep| matches!(dep, Dep::Summary(found, ..) if found == path)));
        }
        inputs.set("leaf.ts", "export class T {}");
        project.invalidate(std::slice::from_ref(&leaf));
        assert!(project.module_symbols(module).unwrap().exports["T"].unwrap().value);
    }
}
