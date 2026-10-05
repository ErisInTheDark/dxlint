//! Boolean Thompson execution with JavaScript boundaries and flat assertions.

use regex_automata::nfa::thompson::{self, State};
use regex_automata::util::look::{Look, LookMatcher};
use regex_automata::PatternID;
use regex_automata::util::primitives::StateID;
use super::translate::Pattern;

pub(super) struct Budget { remaining: usize }

impl Budget {
    pub fn new(remaining: usize) -> Self { Self { remaining } }
    fn step(&mut self) -> Result<(), String> {
        self.remaining = self.remaining.checked_sub(1).ok_or("JavaScript regex work budget exhausted")?;
        Ok(())
    }
}

pub(super) struct Program {
    nfa: thompson::NFA,
    assertions: Vec<Assertion>,
}

struct Assertion { slot: usize, program: Program, positive: bool }

impl Program {
    pub fn compile(pattern: Pattern, flags: &str) -> Result<Self, String> {
        let nfa = thompson::Compiler::new()
            .configure(thompson::Config::new().which_captures(thompson::WhichCaptures::All).nfa_size_limit(Some(8 << 20)))
            .syntax(super::syntax(flags))
            .build(&pattern.source).map_err(|e| format!("invalid JavaScript regex: {e}"))?;
        if nfa.states().len() > 50_000 { return Err("JavaScript regex exceeds 50000 states".into()); }
        let mut assertions = vec![];
        for (index, (inner, positive)) in pattern.assertions.into_iter().enumerate() {
            let program = Self::compile(inner, flags)?;
            // Zero repetitions can erase a marker, but its body still needs syntax validation.
            let Some(group) = nfa.group_info().to_index(PatternID::ZERO, &format!("zlint_assertion_{index}")) else { continue };
            let slot = nfa.group_info().slot(PatternID::ZERO, group).ok_or("JavaScript assertion slot missing")?;
            assertions.push(Assertion { slot, program, positive });
        }
        Ok(Self { nfa, assertions })
    }

    pub fn matches(&self, text: &str, from: usize, anchored: bool, budget: &mut Budget) -> Result<bool, String> {
        let mut seen = vec![0; self.nfa.states().len()];
        let mut seeds = Vec::<StateID>::new();
        let mut active = vec![];
        let look_matcher = LookMatcher::new();
        for at in from..=text.len() {
            if at == from || !anchored && text.is_char_boundary(at) { seeds.push(self.nfa.start_anchored()); }
            while let Some(id) = seeds.pop() {
                if seen[id.as_usize()] == at + 1 { continue; }
                seen[id.as_usize()] = at + 1;
                budget.step()?;
                match self.nfa.state(id) {
                    State::ByteRange { .. } | State::Sparse(_) | State::Dense(_) => active.push(id),
                    State::Look { look, next } => {
                        let holds = match look {
                            Look::StartLF => at == 0 || text.is_char_boundary(at) && text[..at].chars().next_back().is_some_and(line_terminator),
                            Look::EndLF => at == text.len() || text.is_char_boundary(at) && text[at..].chars().next().is_some_and(line_terminator),
                            _ => look_matcher.matches(*look, text.as_bytes(), at),
                        };
                        if holds { seeds.push(*next); }
                    }
                    State::Union { alternates } => seeds.extend(alternates.iter().copied()),
                    State::BinaryUnion { alt1, alt2 } => { seeds.push(*alt1); seeds.push(*alt2); }
                    State::Capture { next, slot, .. } => {
                        let holds = if let Some(assertion) = self.assertions.iter().find(|a| a.slot == slot.as_usize()) {
                            assertion.program.matches(text, at, true, budget)? == assertion.positive
                        } else { true };
                        if holds { seeds.push(*next); }
                    }
                    State::Match { .. } => return Ok(true),
                    State::Fail => {}
                }
            }
            if anchored && active.is_empty() { return Ok(false); }
            if at == text.len() { break; }
            for id in active.drain(..) {
                budget.step()?;
                let next = match self.nfa.state(id) {
                    State::ByteRange { trans } => (trans.start <= text.as_bytes()[at] && text.as_bytes()[at] <= trans.end).then_some(trans.next),
                    State::Sparse(transitions) => transitions.matches(text.as_bytes(), at),
                    State::Dense(transitions) => transitions.matches(text.as_bytes(), at),
                    _ => None,
                };
                if let Some(next) = next { seeds.push(next); }
            }
        }
        Ok(false)
    }
}

fn line_terminator(c: char) -> bool { matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') }
