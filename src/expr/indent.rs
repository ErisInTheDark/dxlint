//! Compiled indentation declarations and per-invocation anchor queries.

use super::{CExpr, Ctx, Value};
use crate::vm::VExpr;
use crate::grammar::{Grammar, Tree, ROOT, TEXT};

/// A rule's declarations and target-specific candidates in precedence order.
pub struct IndentPlan {
    rules: Vec<IndentRule>,
    plain: Vec<Vec<usize>>,
    fields: Vec<Vec<(u16, Vec<usize>)>>,
    /// Each rule line's `from` and `by`, lowered to the VM.
    lowered: Vec<(Option<VExpr>, Option<VExpr>)>,
    /// The rule's `indent reset`, lowered.
    reset: Option<VExpr>,
}

static EMPTY: IndentPlan = IndentPlan::EMPTY;

impl IndentPlan {
    pub const EMPTY: Self = Self { rules: Vec::new(), plain: Vec::new(), fields: Vec::new(), lowered: Vec::new(), reset: None };

    pub fn new(rules: Vec<IndentRule>, grammar: &Grammar) -> Self {
        if rules.is_empty() {
            return Self::EMPTY;
        }
        let mut plan = Self {
            plain: vec![Vec::new(); grammar.matchers.len()],
            fields: vec![Vec::new(); grammar.matchers.len()],
            rules,
            lowered: Vec::new(),
            reset: None,
        };
        for (i, rule) in plan.rules.iter().enumerate() {
            for &(target, field) in &rule.targets {
                for kind in 0..grammar.matchers.len() as u32 {
                    if !grammar.is_a(kind, target) { continue; }
                    let entries = match field {
                        None => &mut plan.plain[kind as usize],
                        Some(label) => {
                            let fields = &mut plan.fields[kind as usize];
                            let at = fields.iter().position(|(f, _)| *f == label).unwrap_or_else(|| {
                                fields.push((label, Vec::new()));
                                fields.len() - 1
                            });
                            &mut fields[at].1
                        }
                    };
                    if entries.last() != Some(&i) { entries.push(i); }
                }
            }
        }
        let rules = &plan.rules;
        let sort = |entries: &mut Vec<usize>| {
            entries.sort_unstable_by_key(|&i| std::cmp::Reverse((rules[i].inside, i)));
            entries.dedup();
        };
        plan.plain.iter_mut().for_each(sort);
        for fields in &mut plan.fields {
            for (_, entries) in fields { sort(entries); }
        }
        plan
    }

    pub(crate) fn specialise(&mut self, options: &[Value<'static>], functions: &[crate::grammar::UserFn]) {
        for rule in &mut self.rules {
            for expr in [&mut rule.by, &mut rule.from].into_iter().flatten() {
                super::fold::specialise(expr, options, functions);
            }
        }
    }

    /// Rule line `i`'s lowered `by`.
    pub(super) fn lowered_by(&self, i: usize) -> Option<&VExpr> {
        self.lowered.get(i).and_then(|l| l.1.as_ref())
    }

    /// Lowers `from`, `by` and the rule's `reset` (after specialisation).
    pub(crate) fn lower(&mut self, reset: Option<&CExpr>, functions: &[crate::grammar::UserFn], values: &mut crate::vm::Values) {
        let mut lower = |e: &Option<CExpr>| e.as_ref().map(|e| crate::vm::lower(e, functions, values));
        self.lowered = self.rules.iter().map(|rule| (lower(&rule.from), lower(&rule.by))).collect();
        self.reset = reset.map(|e| crate::vm::lower(e, functions, values));
    }

    fn candidates(&self, tree: &Tree, node: u32, inside: bool, own: bool) -> Candidates<'_> {
        let kind = tree.node(node).matcher;
        let plain = if kind == ROOT || kind == TEXT { &[][..] } else {
            self.plain.get(kind as usize).map_or(&[][..], Vec::as_slice)
        };
        let fields = tree.label(node).zip(tree.owner(node)).and_then(|(label, owner)| {
            self.fields.get(tree.node(owner).matcher as usize)?
                .iter().find(|(f, _)| *f == label).map(|(_, entries)| entries.as_slice())
        }).unwrap_or(&[]);
        Candidates { plan: self, plain, fields, plain_at: 0, field_at: 0, inside, own }
    }
}

impl std::ops::Deref for IndentPlan {
    type Target = [IndentRule];
    fn deref(&self) -> &Self::Target { &self.rules }
}

/// Merge borrowed lists; a declaration matching both keeps its field priority.
struct Candidates<'a> {
    plan: &'a IndentPlan,
    plain: &'a [usize],
    fields: &'a [usize],
    plain_at: usize,
    field_at: usize,
    inside: bool,
    own: bool,
}

impl Iterator for Candidates<'_> {
    type Item = (bool, usize);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            while self.plain.get(self.plain_at).is_some_and(|i| self.fields.contains(i)) {
                self.plain_at += 1;
            }
            let plain = self.plain.get(self.plain_at).copied();
            let field = self.fields.get(self.field_at).copied();
            let key = |i: usize, qualified: bool| (self.plan.rules[i].inside, qualified, i);
            let i = match (plain, field) {
                (None, None) => return None,
                (Some(p), Some(f)) if key(p, false) > key(f, true) => { self.plain_at += 1; p }
                (_, Some(f)) => { self.field_at += 1; f }
                (Some(p), None) => { self.plain_at += 1; p }
            };
            let inside = self.plan.rules[i].inside;
            if if inside { self.inside } else { self.own } { return Some((inside, i)); }
        }
    }
}

#[cfg(test)]
mod semantic_tests {
    use std::path::Path;

    use crate::config::ast::Severity;
    use crate::engine::tests::native_linter;

    #[test]
    fn hits_do_not_cross_rules_nodes_or_files() {
        let l = native_linter(r#"
            in ts_line rule a info "{indent_level}:{len(indent_anchor)}" { indent in ts_block by 2 }
            in ts_line rule b info "{indent_level}:{len(indent_anchor)}" { indent in ts_block by 5 }
        "#);
        for (path, indent) in [("/root/a.ts", 2), ("/root/b.ts", 6), ("/root/a.ts", 0)] {
            let source = format!("{}{{\n  action();\n}}\n", " ".repeat(indent));
            let got = l.lint(Path::new(path), &source);
            let line: Vec<_> = got.iter().filter(|d| d.start == source.find("  action").unwrap()).collect();
            assert_eq!(line.len(), 2);
            assert_eq!(line[0].message, format!("2:{indent}"));
            assert_eq!(line[1].message, format!("5:{indent}"));
            assert!(got.iter().all(|d| d.severity != Severity::Error));
        }
    }
}

/// A compiled `indent X by n` / `indent in X by n` rule line.
pub struct IndentRule {
    pub inside: bool,
    /// `dedent`: the levels count back from the anchor.
    pub dedent: bool,
    /// (matcher, field) pairs; a field means "what fills that field".
    pub targets: Vec<(u32, Option<u16>)>,
    /// Levels, read on the indented node; 1 when absent.
    pub by: Option<CExpr>,
    /// `from`: an offset (or node), read on the indented node, whose line
    /// is the anchor.
    pub from: Option<CExpr>,
    /// `class`: levels opened on one line collapse within a class (0, the
    /// unnamed one, by default) and add up across classes.
    pub class: u32,
}

/// Where a line's indentation comes from: the node an `indent` line matched,
/// the line it is measured from, and for `in` forms the inside's span.
/// `extra`: levels between that line and an anchor in the middle of it
/// (the anchor's virtual indentation).
#[derive(Clone)]
pub(super) struct IndentHit {
    pub(super) node: u32,
    /// No declaration index when reset replaces its levels.
    pub(super) rule: Option<usize>,
    pub(super) base: AnchorBase,
    pub(super) extra: f64,
    pub(super) inside: Option<(usize, usize)>,
}

/// What an indent line measures from: a line's indentation, or (`from` a
/// text) that text itself, `""` being column 0.
#[derive(Clone)]
pub(super) enum AnchorBase {
    Line(usize),
    Text(String),
}

/// Where an indent line's `from` (or default) puts its anchor.
enum AnchorAt {
    Offset(usize),
    Text(String),
}

/// Mid-line anchors resolve through at most this many indent lines.
const MAX_VIRTUAL_DEPTH: u32 = 32;


impl<'a> Ctx<'a> {
    /// Leading whitespace of `line`, as fixed when it lies above the node.
    pub(super) fn line_indent(&self, line: usize) -> Value<'a> {
        match self.fixed_indent(line) {
            Some(ws) => Value::from(ws),
            None => Value::from(self.indent_at(self.lines.line_start(line))),
        }
    }

    /// The whole of `line`, with its fixed indentation when it lies above
    /// the node.
    pub(super) fn whole_line(&self, line: usize) -> Value<'a> {
        let text = self.lines.line_text(self.source, line);
        match self.fixed_indent(line) {
            Some(ws) => Value::from(format!("{ws}{}", text.trim_start_matches([' ', '\t']))),
            None => Value::from(text),
        }
    }

    fn fixed_indent(&self, line: usize) -> Option<String> {
        let own = self.lines.line_of(self.tree.node(self.node).start as usize);
        if line >= own {
            return None;
        }
        self.reindented.borrow().get(&line).cloned()
    }

    /// Leading whitespace of the line holding byte `at`.
    pub(super) fn indent_at(&self, at: usize) -> &'a str {
        let text = self.lines.line_text(self.source, self.lines.line_of(at));
        &text[..text.len() - text.trim_start_matches([' ', '\t']).len()]
    }

    /// The `indent` line deciding this node's line: walking out from the
    /// line's first character, the innermost node one of the rule's `indent`
    /// lines applies to. `indent X` applies when X starts its own line, below
    /// its owner's line; `indent in X` when X's inside (its `*`, or its text
    /// after its first line) opened on an earlier line and holds this one.
    /// On one node a field-qualified line beats a plain one, then the last.
    pub(super) fn indent_hit(&self) -> Result<Option<IndentHit>, String> {
        if self.indents.is_empty() && self.indent_reset.is_none() {
            return Ok(None);
        }
        if let Some(hit) = self.memo.indent_hits.borrow().get(&self.node) {
            return Ok(hit.clone());
        }
        let hit = self.find_indent_hit()?;
        self.memo.indent_hits.borrow_mut().insert(self.node, hit.clone());
        Ok(hit)
    }

    fn find_indent_hit(&self) -> Result<Option<IndentHit>, String> {
        let start = self.tree.node(self.node).start as usize;
        let line = self.lines.line_of(start);
        let line_start = self.lines.line_start(line);
        let first = line_start + self.indent_at(line_start).len();
        let own_line = |at: usize| self.lines.line_start(self.lines.line_of(at)) + self.indent_at(at).len() == at;
        // Start from the node itself unless it is a layer's root.
        let mut p = Some(self.node);
        while let Some(id) = p {
            // From a named layer, indentation is the default layer's: carry
            // on from the node holding the line's first character there.
            if self.tree.layer_roots.iter().any(|&(r, _)| r == id) || self.tree.layer_of(id).is_some() {
                // Labelled text counts: a line can start with a `?.` field.
                p = Some(self.tree.innermost_with_text(self.grammar, first as u32, self.node)).filter(|&n| !self.tree.is_root(n));
                if p == Some(id) {
                    break;
                }
                continue;
            }
            if self.tree.is_root(id) {
                break;
            }
            let n = self.tree.node(id);
            let label = self.tree.label(id);
            let owner = label.and_then(|_| self.tree.owner(id));
            // Every line that applies here; an `in` form (the node's inside)
            // beats the node form (the node's own start line decides that
            // one), then a field-qualified line, then the later one.
            let (s, e) = (n.start as usize, n.end as usize);
            if s == first && self.indent_reset_at(id)? {
                return Ok(Some(IndentHit { node: id, rule: None, base: AnchorBase::Line(line), extra: 0.0, inside: None }));
            }
            let inside = self.tree.hole(id).map_or((s, e), |(a, b)| (a as usize, b as usize));
            let inside_applies = self.lines.line_of(inside.0) < line && line_start <= inside.1;
            let owner_line = owner
                .or_else(|| self.tree.parent(id))
                .filter(|&o| !self.tree.is_root(o))
                .map(|o| self.lines.line_of(self.tree.node(o).start as usize));
            let node_applies = owner_line.is_some_and(|ol| own_line(s) && ol < self.lines.line_of(s)) && self.lines.line_of(s) <= line && line_start <= e;
            for (in_form, i) in self.indents.candidates(self.tree, id, inside_applies, node_applies) {
                let inside = in_form.then_some(inside);
                let from = self.indent_anchor_at(id, i, in_form)?;
                if matches!(&from, AnchorAt::Offset(at) if self.lines.line_of(*at) >= line) {
                    continue;
                }
                if self.indent_reset_at(id)? {
                    return Ok(Some(IndentHit { node: id, rule: None, base: AnchorBase::Line(line), extra: 0.0, inside }));
                }
                match from {
                    AnchorAt::Text(text) => return Ok(Some(IndentHit { node: id, rule: Some(i), base: AnchorBase::Text(text), extra: 0.0, inside })),
                    AnchorAt::Offset(at) => {
                        let (base, extra) = self.virtual_anchor(at, &mut vec![self.indents[i].class], 0)?;
                        return Ok(Some(IndentHit { node: id, rule: Some(i), base, extra, inside }));
                    }
                }
            }
            p = self.tree.parent(id);
        }
        let first_node = self.tree.innermost_with_text(self.grammar, first as u32, self.node);
        if !self.tree.is_root(first_node) && self.indent_reset_at(first_node)? {
            return Ok(Some(IndentHit { node: first_node, rule: None, base: AnchorBase::Line(line), extra: 0.0, inside: None }));
        }
        Ok(None)
    }

    /// An indent expression's value on `on.node`, through the VM when on (an
    /// error falls back to the interpreter, which reports it).
    pub(super) fn indent_eval(on: &Ctx<'a>, e: &CExpr, lowered: Option<&VExpr>) -> Result<Value<'a>, String> {
        if crate::vm::mode() == crate::vm::Mode::On {
            if let Some(v) = lowered.and_then(|vm| crate::vm::eval(on, on.node, vm)) {
                return Ok(v.value(on, on.source));
            }
            crate::vm::note_fallback("indent");
        }
        on.eval(e)
    }

    fn indent_reset_at(&self, node: u32) -> Result<bool, String> {
        match self.indent_reset {
            Some(reset) => {
                let on = Ctx { node, indents: &EMPTY, indent_reset: None, locals: None, ..*self };
                Ok(Self::indent_eval(&on, reset, self.indents.reset.as_ref())?.truthy())
            }
            None => Ok(false),
        }
    }

    /// Where indent line `i` on node `id` measures from: its `from` (an
    /// offset, a node, or a text to measure from directly), else the node's
    /// start (`in` form) or its owner's (node form). `from` is read on the
    /// node with no indent lines in scope, so it cannot ask for indentation
    /// itself; null means the default.
    fn indent_anchor_at(&self, id: u32, i: usize, in_form: bool) -> Result<AnchorAt, String> {
        let from = match self.indents[i].from.as_ref() {
            Some(from) => match Self::indent_eval(&Ctx { node: id, indents: &EMPTY, indent_reset: None, ..*self }, from, self.indents.lowered.get(i).and_then(|l| l.0.as_ref()))? {
                Value::Node(n, _) => Some(AnchorAt::Offset(self.tree.node(n).start as usize)),
                Value::Null => None,
                Value::Str(text) => Some(AnchorAt::Text(text.to_string())),
                v => Some(AnchorAt::Offset((v.num()?.max(0.0) as usize).min(self.source.len()))),
            },
            None => None,
        };
        Ok(from.unwrap_or_else(|| {
            let owner = if in_form { None } else { self.tree.label(id).and_then(|_| self.tree.owner(id)).or_else(|| self.tree.parent(id)) };
            AnchorAt::Offset(self.tree.node(owner.unwrap_or(id)).start as usize)
        }))
    }

    /// What an anchor at byte `at` stands for, and the levels on top of it.
    /// An anchor in the middle of a line sits where the indent line deciding
    /// it puts it, when that line measures from a text or from earlier on
    /// the same line (so this ends); levels opened on one line collapse, so
    /// it adds levels only for a class not counted on the line yet (`seen`,
    /// starting with the indented line's own). Anything else stands for its
    /// line, whose first token answers for what is above, unless the line
    /// starts inside a token: it continues the line above, as one line.
    fn virtual_anchor(&self, at: usize, seen: &mut Vec<u32>, depth: u32) -> Result<(AnchorBase, f64), String> {
        let line = self.lines.line_of(at);
        let line_start = self.lines.line_start(line);
        if depth >= MAX_VIRTUAL_DEPTH || line_start + self.indent_at(line_start).len() >= at {
            return Ok((AnchorBase::Line(line), 0.0));
        }
        let mut p = Some(self.tree.innermost_with_text(self.grammar, at as u32, u32::MAX)).filter(|&n| !self.tree.is_root(n));
        while let Some(id) = p.filter(|&n| !self.tree.is_root(n)) {
            let n = self.tree.node(id);
            let (s, e) = (n.start as usize, n.end as usize);
            let inside = self.tree.hole(id).map_or((s, e), |(a, b)| (a as usize, b as usize));
            // The node's own first token is not inside it.
            let inside_applies = inside.0 <= at && at < inside.1 && at > s;
            let node_applies = s <= at && at < e;
            for (in_form, i) in self.indents.candidates(self.tree, id, inside_applies, node_applies) {
                let from = self.indent_anchor_at(id, i, in_form)?;
                if matches!(&from, AnchorAt::Offset(from) if *from >= at) {
                    continue;
                }
                if self.indent_reset_at(id)? {
                    return Ok((AnchorBase::Line(line), 0.0));
                }
                // A line starting inside a token (template text, a string,
                // a block comment) has no code indentation to stand for:
                // it continues the line above, so follow the anchor up.
                if matches!(&from, AnchorAt::Offset(from) if self.lines.line_of(*from) < line)
                    && !self.native.is_some_and(|facts| facts.inside_token(line_start))
                {
                    return Ok((AnchorBase::Line(line), 0.0));
                }
                let rule = &self.indents[i];
                let counts = !seen.contains(&rule.class);
                if counts {
                    seen.push(rule.class);
                }
                let levels = match (&rule.by, counts) {
                    (_, false) => 0.0,
                    (Some(by), true) => Self::indent_eval(&Ctx { node: id, ..*self }, by, self.indents.lowered.get(i).and_then(|l| l.1.as_ref()))?.num()?,
                    (None, true) => 1.0,
                };
                let levels = if rule.dedent { -levels } else { levels };
                return Ok(match from {
                    AnchorAt::Text(text) => (AnchorBase::Text(text), levels),
                    AnchorAt::Offset(from) => {
                        let (base, extra) = self.virtual_anchor(from, seen, depth + 1)?;
                        (base, extra + levels)
                    }
                });
            }
            p = self.tree.parent(id);
        }
        Ok((AnchorBase::Line(line), 0.0))
    }

    /// This line starts with the end of the `in` form's inside (its closer),
    /// so it sits at the anchor.
    pub(super) fn indent_closes(&self, hit: &IndentHit) -> bool {
        let Some((_, end)) = hit.inside else { return false };
        let start = self.tree.node(self.node).start as usize;
        let line = self.lines.line_of(start);
        let line_start = self.lines.line_start(line);
        if self.lines.line_of(end) != line {
            return false;
        }
        let first = line_start + self.indent_at(line_start).len();
        // An `until` ends where its closer starts; a node without one ends
        // after its closing brackets.
        match self.tree.hole(hit.node) {
            Some(_) => self.source[line_start..end].trim().is_empty(),
            None => first <= end && self.source[first..end].chars().all(|c| matches!(c, ')' | ']' | '}' | '>')) && first < end,
        }
    }

}
