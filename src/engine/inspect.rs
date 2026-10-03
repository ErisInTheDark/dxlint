//! What zlint sees at one position of a source file, for writing rules: the
//! layers covering it, the field each sits in, and the native facts of the
//! innermost native node. The language server shows it as a hover.

use std::fmt::Write as _;
use std::path::Path;

use super::Linter;
use crate::expr::{facts, show};
use crate::grammar::{Tree, ROOT, TEXT};
use crate::native::ts::facts::NativeFile;
use crate::native::ts::syntax::Syntax;

/// At most this many characters of a node's text are shown.
const SNIPPET: usize = 40;

impl Linter {
    /// Markdown describing the layers at byte `offset`, innermost first;
    /// `None` when the file is not linted at all.
    pub fn inspect(&self, path: &Path, text: &str, offset: usize) -> Option<String> {
        let rel = self.relative(path);
        let lang = self.grammar.lang_for(&rel)?;
        let mut out = String::new();
        // Every overlay: the point is to see everything.
        let overlays = vec![true; self.grammar.matchers.len()];
        let tree = Tree::parse_with(&self.grammar, lang, text, Some(&overlays), None);
        self.layers_at(&tree, text, offset, &mut out);
        if let (Some(front), Some(jsx)) = (self.grammar.natives.first(), crate::native::ts::reads(path)) {
            if let Ok(parsed) = crate::native::ts::parse(text, jsx) {
                let tree = Tree::from_native(front.remap(parsed.nodes), parsed.root);
                let chain = self.layers_at(&tree, text, offset, &mut out);
                if let Some(&node) = chain.first() {
                    let file = NativeFile::new(Syntax::new(&tree, text, front), path, &self.project);
                    let known: Vec<String> = facts::FACTS
                        .iter()
                        .filter_map(|f| {
                            let v = (f.get)(&file, node);
                            (!matches!(v, crate::expr::Value::Null)).then(|| format!("`{}` = {}", f.name, short(&show(&v))))
                        })
                        .collect();
                    if !known.is_empty() {
                        let _ = write!(out, "\nfacts of `{}`:\n\n- {}\n", self.grammar.matchers[tree.node(node).matcher as usize].name, known.join("\n- "));
                    }
                }
            }
        }
        (!out.is_empty()).then_some(out)
    }

    /// Lists the layer nodes of `tree` covering `offset`, innermost first;
    /// returns their ids.
    fn layers_at(&self, tree: &Tree, text: &str, offset: usize, out: &mut String) -> Vec<u32> {
        let mut chain = Vec::new();
        let mut at = tree.root;
        'down: loop {
            for e in &tree.node(at).children {
                let n = tree.node(e.node);
                if (n.start as usize) <= offset && offset < (n.end as usize).max(n.start as usize + 1) {
                    if n.matcher != TEXT {
                        chain.push(e.node);
                    }
                    at = e.node;
                    continue 'down;
                }
            }
            break;
        }
        chain.reverse();
        for &id in &chain {
            let n = tree.node(id);
            if n.matcher == ROOT {
                continue;
            }
            let field = match (tree.label(id), tree.owner(id)) {
                (Some(l), Some(o)) => format!(" — `{}` of `{}`", self.grammar.labels.name(l), self.grammar.matchers[tree.node(o).matcher as usize].name),
                _ => String::new(),
            };
            let name = &self.grammar.matchers[n.matcher as usize].name;
            let _ = writeln!(out, "- `{name}`{field}: `{}`", short(&text[n.start as usize..n.end as usize]));
        }
        chain
    }
}

/// A one-line, shortened text.
fn short(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > SNIPPET {
        format!("{}…", one.chars().take(SNIPPET).collect::<String>())
    } else {
        one.replace('`', "'")
    }
}
