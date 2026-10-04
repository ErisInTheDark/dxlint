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
];

fn declaration(file: &NativeFile<'_>, node: u32) -> Vec<u32> {
    let b = file.bindings();
    if b.decls.contains_key(&node) {
        return vec![node];
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
