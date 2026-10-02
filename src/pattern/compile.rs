//! Lowers a [`Pat`] into a flat instruction [`Program`].

use std::collections::HashMap;

use super::class::CharClass;
use super::dfa::LookDfa;
use super::prefilter::ByteSet;
use super::{Look, Pat};

const MAX_INSTS: usize = 50_000;
pub const NO_LABEL: u16 = u16::MAX;

#[derive(Clone, Copy, Debug)]
pub enum Inst {
    Lit(u32),
    LitFold(u32),
    Class(u32),
    Any,
    AnyNoNl,
    /// Try the first target, backtrack into the second.
    Split(u32, u32),
    Jmp(u32),
    /// Text capture boundary: slot `2 * label` opens, `2 * label + 1` closes.
    Save(u16),
    /// Where a `*` starts (`false`) and ends (`true`): the layer's inside,
    /// which `in layer indent` indents.
    Hole(bool),
    Look(Look),
    /// Match another matcher here, as a child layer under `label`.
    Call { matcher: u32, label: u16 },
    /// One step of `*`: eat the longest child of the running matcher, or one
    /// char if no child matches, then fast-forward past bytes where neither
    /// a child nor the continuation can start (`Program::skip_stops`).
    Skip(u32),
    /// Zero-width lookaround, Program::arounds[i].
    Around(u32),
    Match,
}

#[derive(Debug, Default)]
pub struct Program {
    pub insts: Vec<Inst>,
    pub lits: Vec<Box<[u8]>>,
    pub classes: Vec<CharClass>,
    /// Per `Skip`: bytes where what follows the `*` could start. `None`
    /// means it could start anywhere (no fast-forward). Filled in by the
    /// grammar, which knows every matcher's first bytes.
    pub skip_stops: Vec<Option<ByteSet>>,
    pub arounds: Vec<LookDfa>,
}

impl Program {
    /// Bytes that can start a match from `pc`, or `None` if it can match
    /// without consuming anything (or the answer is "anything").
    pub fn first_from(&self, pc: usize, call_first: &dyn Fn(u32) -> ByteSet) -> Option<ByteSet> {
        let mut set = ByteSet::default();
        let mut seen = vec![false; self.insts.len()];
        let mut stack = vec![pc];
        while let Some(pc) = stack.pop() {
            if std::mem::replace(&mut seen[pc], true) {
                continue;
            }
            match self.insts[pc] {
                Inst::Lit(i) => set.insert(self.lits[i as usize][0]),
                Inst::LitFold(i) => {
                    let b = self.lits[i as usize][0];
                    set.insert(b.to_ascii_lowercase());
                    set.insert(b.to_ascii_uppercase());
                }
                Inst::Class(i) => {
                    let class = &self.classes[i as usize];
                    (0..128u8).filter(|&b| class.contains_ascii(b)).for_each(|b| set.insert(b));
                    if class.may_match_non_ascii() {
                        (0xC0..=0xFFu8).for_each(|b| set.insert(b));
                    }
                }
                Inst::Any | Inst::AnyNoNl | Inst::Skip(_) | Inst::Match => return None,
                Inst::Split(a, b) => {
                    stack.push(a as usize);
                    stack.push(b as usize);
                }
                Inst::Jmp(t) => stack.push(t as usize),
                Inst::Save(_) | Inst::Hole(_) => stack.push(pc + 1),
                // `$` only holds right before a line break (or at the end,
                // where fast-forwarding stops anyway).
                Inst::Look(Look::Eol) => {
                    set.insert(b'\r');
                    set.insert(b'\n');
                }
                Inst::Look(Look::Eof) => {}
                Inst::Look(_) | Inst::Around(_) => stack.push(pc + 1),
                Inst::Call { matcher, .. } => set.union(&call_first(matcher)),
            }
        }
        Some(set)
    }
}

/// Interned field labels, shared by every matcher of a grammar.
#[derive(Debug, Default, Clone)]
pub struct Labels {
    names: Vec<String>,
}

impl Labels {
    pub fn get(&self, name: &str) -> Option<u16> {
        self.names.iter().position(|n| n == name).map(|i| i as u16)
    }

    pub fn intern(&mut self, name: &str) -> u16 {
        self.get(name).unwrap_or_else(|| {
            self.names.push(name.to_string());
            self.names.len() as u16 - 1
        })
    }

    pub fn name(&self, id: u16) -> &str {
        &self.names[id as usize]
    }
}

pub fn compile(pat: &Pat, matchers: &HashMap<String, u32>, labels: &mut Labels) -> Result<Program, String> {
    let mut c = Compiler { prog: Program::default(), matchers, labels };
    c.emit(pat)?;
    c.push(Inst::Match)?;
    Ok(c.prog)
}

struct Compiler<'a> {
    prog: Program,
    matchers: &'a HashMap<String, u32>,
    labels: &'a mut Labels,
}

impl Compiler<'_> {
    fn pc(&self) -> u32 {
        self.prog.insts.len() as u32
    }

    fn push(&mut self, inst: Inst) -> Result<u32, String> {
        if self.prog.insts.len() >= MAX_INSTS {
            return Err(format!("pattern is too large (over {MAX_INSTS} instructions); reduce counted repetitions"));
        }
        self.prog.insts.push(inst);
        Ok(self.pc() - 1)
    }

    fn patch(&mut self, at: u32, inst: Inst) {
        self.prog.insts[at as usize] = inst;
    }

    fn emit(&mut self, pat: &Pat) -> Result<(), String> {
        match pat {
            Pat::Empty => {}
            Pat::Lit { bytes, fold } => {
                if !bytes.is_empty() {
                    let idx = self.prog.lits.len() as u32;
                    self.prog.lits.push(bytes.clone().into_boxed_slice());
                    self.push(if *fold { Inst::LitFold(idx) } else { Inst::Lit(idx) })?;
                }
            }
            Pat::Class(class) => {
                let idx = self.prog.classes.len() as u32;
                self.prog.classes.push(class.clone());
                self.push(Inst::Class(idx))?;
            }
            Pat::Any { newline } => {
                self.push(if *newline { Inst::Any } else { Inst::AnyNoNl })?;
            }
            Pat::Look(look) => {
                self.push(Inst::Look(*look))?;
            }
            Pat::Seq(items) => {
                for item in items {
                    self.emit(item)?;
                }
            }
            Pat::Alt(items) => self.emit_alt(items)?,
            Pat::Capture { name, inner } => {
                let label = self.labels.intern(name);
                self.push(Inst::Save(label * 2))?;
                self.emit(inner)?;
                self.push(Inst::Save(label * 2 + 1))?;
            }
            Pat::Repeat { inner, min, max, greedy } => self.emit_repeat(inner, *min, *max, *greedy)?,
            Pat::Ref { name, label } => {
                let matcher = *self.matchers.get(name).ok_or_else(|| format!("unknown matcher `{name}`"))?;
                let label = label.as_deref().map_or(NO_LABEL, |l| self.labels.intern(l));
                self.push(Inst::Call { matcher, label })?;
            }
            Pat::Around { behind, negate, inner } => {
                let idx = self.prog.arounds.len() as u32;
                self.prog.arounds.push(LookDfa::new(inner, *behind, *negate)?);
                self.push(Inst::Around(idx))?;
            }
            Pat::Skip => {
                // Lazy loop: try what follows first, else take one more step.
                self.push(Inst::Hole(false))?;
                let split = self.push(Inst::Split(0, 0))?;
                self.push(Inst::Skip(self.prog.skip_stops.len() as u32))?;
                self.prog.skip_stops.push(None);
                self.push(Inst::Jmp(split))?;
                let end = self.pc();
                self.patch(split, Inst::Split(end, split + 1));
                self.push(Inst::Hole(true))?;
            }
        }
        Ok(())
    }

    fn emit_alt(&mut self, items: &[Pat]) -> Result<(), String> {
        let mut jumps = Vec::new();
        for (i, item) in items.iter().enumerate() {
            if i + 1 < items.len() {
                let split = self.push(Inst::Split(0, 0))?;
                self.emit(item)?;
                jumps.push(self.push(Inst::Jmp(0))?);
                let next = self.pc();
                self.patch(split, Inst::Split(split + 1, next));
            } else {
                self.emit(item)?;
            }
        }
        let end = self.pc();
        for j in jumps {
            self.patch(j, Inst::Jmp(end));
        }
        Ok(())
    }

    fn emit_optional(&mut self, inner: &Pat, greedy: bool) -> Result<(), String> {
        let split = self.push(Inst::Split(0, 0))?;
        self.emit(inner)?;
        let end = self.pc();
        self.patch(split, if greedy { Inst::Split(split + 1, end) } else { Inst::Split(end, split + 1) });
        Ok(())
    }

    fn emit_repeat(&mut self, inner: &Pat, min: u32, max: Option<u32>, greedy: bool) -> Result<(), String> {
        for _ in 0..min {
            self.emit(inner)?;
        }
        match max {
            None => {
                let split = self.push(Inst::Split(0, 0))?;
                self.emit(inner)?;
                self.push(Inst::Jmp(split))?;
                let end = self.pc();
                self.patch(split, if greedy { Inst::Split(split + 1, end) } else { Inst::Split(end, split + 1) });
            }
            Some(max) => {
                for _ in min..max {
                    self.emit_optional(inner, greedy)?;
                }
            }
        }
        Ok(())
    }
}
