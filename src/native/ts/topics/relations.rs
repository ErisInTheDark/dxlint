//! Links between nodes that rules navigate (`declaration.line`): bindings
//! and their uses. What a rule makes of them is written in the rule.

use std::collections::HashMap;

use crate::expr::facts::Link;
use crate::native::ts::facts::NativeFile;

pub const LINKS: &[Link] = &[
    Link {
        name: "declaration",
        doc: "native layers: on a referencing identifier, the identifier that declares its binding (a declaring one gives itself); null when unresolved",
        many: false,
        get: |f, n| f.nodes_of(declaration, n),
    },
    Link {
        name: "references",
        doc: "native layers: on a declaring identifier, every identifier referencing its binding (reads, writes, type uses), in source order",
        many: true,
        get: |f, n| f.nodes_of(references, n),
    },
    Link {
        name: "declarations",
        doc: "native layers: on an identifier of a binding, every identifier declaring it (`var a; var a = 1;`, overloads, class + interface), in source order",
        many: true,
        get: |f, n| f.nodes_of(declarations, n),
    },
    Link {
        name: "scope",
        doc: "native layers: on a declaring or referencing identifier, the node opening the scope it sits in (program, function, block, class, `for`, catch clause...)",
        many: false,
        get: |f, n| f.nodes_of(scope, n),
    },
    Link {
        name: "conditional_ancestor",
        doc: "native layers: the `if`, loop or `?:` whose test holds the node, not across functions; null outside tests",
        many: false,
        get: |f, n| f.nodes_of(|file, node| file.conditional_ancestor(node).into_iter().collect(), n),
    },
    Link {
        name: "writer",
        doc: "native layers: on a written identifier, member or pattern, the node writing it (assignment, update, `delete`, `for` statement); null when not written",
        many: false,
        get: |f, n| f.nodes_of(writer, n),
    },
];

/// Every identifier declaring the binding `node` declares or references.
fn declarations(file: &NativeFile<'_>, node: u32) -> Vec<u32> {
    let b = file.bindings();
    let Some(&first) = declaration(file, node).first() else { return Vec::new() };
    let first = b.merged.get(&first).copied().unwrap_or(first);
    let mut out: Vec<u32> = std::iter::once(first).chain(b.merged.iter().filter(|&(_, &f)| f == first).map(|(&d, _)| d)).collect();
    out.sort_by_key(|&n| file.syntax.tree.node(n).start);
    out.dedup();
    out
}

fn scope(file: &NativeFile<'_>, node: u32) -> Vec<u32> {
    let b = file.bindings();
    b.decl_scope.get(&node).or_else(|| b.ref_scope.get(&node)).map(|&i| b.scopes[i].node).into_iter().collect()
}

fn writer(file: &NativeFile<'_>, node: u32) -> Vec<u32> {
    file.bindings().write_sites.get(&node).copied().into_iter().collect()
}

fn declaration(file: &NativeFile<'_>, node: u32) -> Vec<u32> {
    let b = file.bindings();
    if b.decls.contains_key(&node) {
        return vec![node];
    }
    // A later declaration merged into an earlier one (`var a; var a;`).
    if let Some(&first) = b.merged.get(&node) {
        return vec![first];
    }
    b.refs.get(&node).copied().into_iter().collect()
}

/// Declaring identifier -> its references, in source order.
struct Uses(HashMap<u32, Vec<u32>>);

fn uses(file: &NativeFile<'_>) -> Uses {
    let s = &file.syntax;
    let mut map: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&reference, &decl) in &file.bindings().refs {
        map.entry(decl).or_default().push(reference);
    }
    for list in map.values_mut() {
        list.sort_by_key(|&n| s.tree.node(n).start);
    }
    Uses(map)
}

fn references(file: &NativeFile<'_>, node: u32) -> Vec<u32> {
    file.cached(uses).0.get(&node).cloned().unwrap_or_default()
}
