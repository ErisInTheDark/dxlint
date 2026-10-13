//! Transparent production outputs become concrete public field shapes.

use std::collections::{BTreeSet, HashMap};

use crate::pattern::syntax::{Definition, Expr, TokenMatch};

#[derive(Clone, Default, PartialEq, Eq)]
struct Output {
    kinds: BTreeSet<usize>,
    text: bool,
}

impl Output {
    fn merge(&mut self, other: Self) {
        self.kinds.extend(other.kinds);
        self.text |= other.text;
    }
}

struct Shapes<'a> {
    kinds: HashMap<&'a str, usize>,
    productions: HashMap<&'a str, usize>,
}

impl Shapes<'_> {
    fn kind(&self, name: &str) -> Result<Output, String> {
        let id = self.kinds.get(name).ok_or_else(|| format!("unknown node `{name}`"))?;
        Ok(Output { kinds: BTreeSet::from([*id]), text: false })
    }

    fn token(&self, token: &TokenMatch) -> Result<Output, String> {
        match &token.emit {
            Some(kind) => self.kind(kind),
            None => Ok(Output { text: true, ..Output::default() }),
        }
    }

    fn output(&self, expression: &Expr, outputs: &[Output], captures: &HashMap<String, Output>) -> Result<Output, String> {
        match expression {
            Expr::Token(token) => self.token(token),
            Expr::Text(_) => Ok(Output { text: true, ..Output::default() }),
            Expr::Scope { capture, header, body } => {
                let header = self.output(header, outputs, captures)?;
                let mut captures = captures.clone();
                captures.insert(capture.clone(), header);
                self.output(body, outputs, &captures)
            }
            Expr::Reuse(name) => captures.get(name).cloned()
                .ok_or_else(|| format!("private capture `{name}` is outside its scope")),
            Expr::TokenPattern { emit, .. } => match emit {
                Some(kind) => self.kind(kind),
                None => Ok(Output { text: true, ..Output::default() }),
            },
            Expr::Call { production, .. } => {
                let id = self.productions.get(production.as_str()).ok_or_else(|| format!("unknown production `{production}`"))?;
                Ok(outputs[*id].clone())
            }
            Expr::Seq(items) | Expr::Choice(items) => {
                let mut output = Output::default();
                for item in items { output.merge(self.output(item, outputs, captures)?); }
                Ok(output)
            }
            Expr::Repeat { inner, .. } | Expr::Bind { inner, .. } | Expr::Context { inner, .. } | Expr::Try(inner) => self.output(inner, outputs, captures),
            Expr::Emit { kind, .. } => self.kind(kind),
            Expr::Flag { yes, no, .. } => {
                let mut output = self.output(yes, outputs, captures)?;
                output.merge(self.output(no, outputs, captures)?);
                Ok(output)
            }
            Expr::Terminator { explicit, .. } => self.token(explicit),
            Expr::Pratt(precedence) => {
                let mut output = self.output(&precedence.atom, outputs, captures)?;
                for prefix in &precedence.prefix { output.merge(self.kind(&prefix.kind)?); }
                for row in &precedence.rows { output.merge(self.kind(&row.kind)?); }
                Ok(output)
            }
            Expr::Discard(_) | Expr::Look { .. } | Expr::Cut | Expr::SameLine | Expr::SameText { .. } | Expr::End => Ok(Output::default()),
        }
    }
}

pub(super) fn resolve(definition: &Definition) -> Result<Definition, String> {
    let shapes = Shapes {
        kinds: definition.nodes.iter().enumerate().map(|(id, node)| (node.name.as_str(), id)).collect(),
        productions: definition.productions.iter().enumerate().map(|(id, production)| (production.name.as_str(), id)).collect(),
    };
    let mut outputs = vec![Output::default(); definition.productions.len()];
    loop {
        let mut changed = false;
        for (id, production) in definition.productions.iter().enumerate() {
            let output = shapes.output(&production.body, &outputs, &HashMap::new())?;
            if outputs[id] != output {
                outputs[id] = output;
                changed = true;
            }
        }
        if !changed { break; }
    }
    let mut resolved = definition.clone();
    for node in &mut resolved.nodes {
        for field in &mut node.fields {
            let mut output = Output { text: field.text, ..Output::default() };
            for kind in &field.kinds {
                if shapes.kinds.contains_key(kind.as_str()) {
                    output.merge(shapes.kind(kind)?);
                } else if let Some(production) = shapes.productions.get(kind.as_str()) {
                    output.merge(outputs[*production].clone());
                } else {
                    return Err(format!("unknown field kind or production `{kind}`"));
                }
            }
            field.kinds = output.kinds.into_iter().map(|id| definition.nodes[id].name.clone()).collect();
            field.text = output.text;
        }
    }
    Ok(resolved)
}
