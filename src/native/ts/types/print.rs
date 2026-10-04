//! Types printed roughly as TypeScript prints them.

use super::super::modules::DeclRef;
use super::{Checker, Ty, MAX_DEPTH};

impl Checker<'_> {
    pub fn decl_name(&self, sym: &[DeclRef]) -> Option<Box<str>> {
        let r = sym.first()?;
        Some(self.summary_of(r.file)?.decls[r.decl as usize].name.clone())
    }

    /// The type as TypeScript would print it, roughly; `?` for parts it
    /// does not know.
    pub fn print(&self, t: &Ty) -> String {
        if self.depth.get() >= MAX_DEPTH {
            return "?".into();
        }
        self.depth.set(self.depth.get() + 1);
        let s = match t {
            Ty::Unknown => "?".into(),
            Ty::IntrinsicUnknown => "unknown".into(),
            Ty::Any => "any".into(),
            Ty::Never => "never".into(),
            Ty::Void => "void".into(),
            Ty::Undefined => "undefined".into(),
            Ty::Null => "null".into(),
            Ty::Prim(p) => (*p).into(),
            Ty::Lit(text, "string") => super::super::regex::cook(text).and_then(|text| serde_json::to_string(&text).ok()).unwrap_or_else(|| text.to_string()),
            Ty::Lit(text, _) => text.to_string(),
            Ty::Named(sym, args) => {
                let name = self.qualified_name(sym);
                if args.is_empty() {
                    name
                } else {
                    format!("{name}<{}>", args.iter().map(|a| self.print(a)).collect::<Vec<_>>().join(", "))
                }
            }
            Ty::Union(items) => {
                let rank = |ty: &Ty| match ty {
                    Ty::Prim("string") => 0,
                    Ty::Prim("number") => 1,
                    Ty::Prim("bigint") => 2,
                    Ty::Prim("boolean") => 3,
                    Ty::Prim("symbol") => 4,
                    Ty::Void => 5,
                    Ty::Undefined => 6,
                    Ty::Null => 7,
                    _ => 8,
                };
                let mut items: Vec<_> = items.iter().collect();
                items.sort_by_key(|ty| rank(ty));
                items.into_iter().map(|i| self.print(i)).collect::<Vec<_>>().join(" | ")
            }
            Ty::Inter(items) => items.iter().map(|i| self.print(i)).collect::<Vec<_>>().join(" & "),
            Ty::Array(e) => {
                let inner = self.print(&e.element);
                let prefix = if e.readonly { "readonly " } else { "" };
                if matches!(*e.element, Ty::Union(_) | Ty::Inter(_) | Ty::Fn(_)) {
                    format!("{prefix}({inner})[]")
                } else {
                    format!("{prefix}{inner}[]")
                }
            }
            Ty::Tuple(items) => format!("{}[{}]", if items.readonly { "readonly " } else { "" }, items.elements.iter().map(|element| {
                format!("{}{}{}", if element.rest { "..." } else { "" }, self.print(&element.ty), if element.optional { "?" } else { "" })
            }).collect::<Vec<_>>().join(", ")),
            Ty::Fn(_) => "Function".into(),
            Ty::Obj(members, env) => {
                let properties: Vec<_> = members.iter().map(|member| {
                    let ty = member.ty.as_ref().map_or(Ty::Unknown, |syn| self.eval(syn, env));
                    format!("{}{}: {}", member.name, if member.optional { "?" } else { "" }, self.print(&ty))
                }).collect();
                if properties.is_empty() { "{}".into() } else { format!("{{ {}; }}", properties.join("; ")) }
            }
            Ty::Struct(props) if props.is_empty() => "{}".into(),
            Ty::Struct(props) => format!("{{ {}; }}", props.iter().map(|(n, t)| format!("{n}: {}", self.print(t))).collect::<Vec<_>>().join("; ")),
            Ty::Class(sym) => format!("typeof {}", self.qualified_name(sym)),
            Ty::Ns(sym) => format!("typeof {}", self.qualified_name(sym)),
            Ty::Module(_) => "module".into(),
            Ty::Param(n) => n.to_string(),
        };
        self.depth.set(self.depth.get() - 1);
        s
    }

    /// `NodeJS.Timeout`: a declaration's name with its namespaces.
    fn qualified_name(&self, sym: &[DeclRef]) -> String {
        let Some(&r) = sym.first() else { return "?".into() };
        let Some(s) = self.summary_of(r.file) else { return "?".into() };
        let mut parts = vec![s.decls[r.decl as usize].name.to_string()];
        let mut p = s.decls[r.decl as usize].parent;
        while let Some(ns) = p {
            parts.push(s.decls[ns as usize].name.to_string());
            p = s.decls[ns as usize].parent;
        }
        parts.reverse();
        parts.join(".")
    }
}
