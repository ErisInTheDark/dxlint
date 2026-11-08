//! Regexes with lookaround groups (`(?=...)`, `(?!...)`, `(?<=...)`,
//! `(?<!...)`), which the linear-time engine lacks: a Thompson simulation
//! whose marker captures stand for the groups, each checked at its position
//! by an ordinary regex. Groups do not nest; a leading flags group
//! (`(?i)`) applies inside them too. Work is bounded.

use regex_automata::meta;
use regex_automata::nfa::thompson::{self, State};
use regex_automata::util::look::LookMatcher;
use regex_automata::util::primitives::StateID;
use regex_automata::{Anchored, Input, PatternID};

/// Steps a match may take before it fails rather than answer.
const BUDGET: usize = 10_000_000;

pub struct Lookaround {
    nfa: thompson::NFA,
    groups: Vec<Group>,
}

struct Group {
    slot: usize,
    check: meta::Regex,
    behind: bool,
    positive: bool,
}

/// Whether `pattern` holds a lookaround group outside classes and escapes.
pub fn has_lookaround(pattern: &str) -> bool {
    split(pattern).is_ok_and(|(_, groups)| !groups.is_empty())
}

/// `pattern` with each lookaround group replaced by a marker capture, and
/// the groups (body, behind, positive).
fn split(pattern: &str) -> Result<(String, Vec<(String, bool, bool)>), String> {
    let mut out = String::with_capacity(pattern.len());
    let mut groups = Vec::new();
    let mut chars = pattern.char_indices().peekable();
    let mut class = false;
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' => {
                out.push(c);
                if let Some((_, next)) = chars.next() { out.push(next); }
            }
            '[' => { class = true; out.push(c); }
            ']' => { class = false; out.push(c); }
            '(' if !class => {
                let rest = &pattern[at..];
                let head = ["(?=", "(?!", "(?<=", "(?<!"].into_iter().find(|head| rest.starts_with(head));
                let Some(head) = head else { out.push(c); continue };
                for _ in 1..head.len() { chars.next(); }
                let body = body(&mut chars)?;
                if split(&body)?.1.len() > 0 { return Err("lookaround groups cannot nest".into()); }
                out.push_str(&format!("(?P<dx_look_{}>)", groups.len()));
                groups.push((body, head.len() == 4, !head.ends_with('!')));
                if groups.len() > 64 { return Err("a regex holds at most 64 lookaround groups".into()); }
            }
            _ => out.push(c),
        }
    }
    Ok((out, groups))
}

/// The text up to the `)` closing a group whose head was just read.
fn body(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>) -> Result<String, String> {
    let mut body = String::new();
    let (mut depth, mut class) = (1, false);
    while let Some((_, c)) = chars.next() {
        match c {
            '\\' => {
                body.push(c);
                body.push(chars.next().ok_or("unterminated escape")?.1);
                continue;
            }
            '[' => class = true,
            ']' => class = false,
            '(' if !class => depth += 1,
            ')' if !class => {
                depth -= 1;
                if depth == 0 { return Ok(body); }
            }
            _ => {}
        }
        body.push(c);
    }
    Err("unterminated lookaround group".into())
}

impl Lookaround {
    pub fn compile(pattern: &str) -> Result<Self, String> {
        let (source, groups) = split(pattern)?;
        let flags = leading_flags(pattern);
        let nfa = thompson::Compiler::new()
            .configure(thompson::Config::new().which_captures(thompson::WhichCaptures::All).nfa_size_limit(Some(8 << 20)))
            .build(&source).map_err(|e| format!("invalid regex `{pattern}`: {e}"))?;
        let mut compiled = Vec::with_capacity(groups.len());
        for (index, (body, behind, positive)) in groups.into_iter().enumerate() {
            let check = if behind { format!("{flags}(?:{body})\\z") } else { format!("{flags}(?:{body})") };
            let check = meta::Regex::new(&check).map_err(|e| format!("invalid regex `{pattern}`: {e}"))?;
            // A group repeated zero times leaves no marker: nothing to check.
            let Some(group) = nfa.group_info().to_index(PatternID::ZERO, &format!("dx_look_{index}")) else { continue };
            let slot = nfa.group_info().slot(PatternID::ZERO, group).ok_or("lookaround slot missing")?;
            compiled.push(Group { slot, check, behind, positive });
        }
        Ok(Self { nfa, groups: compiled })
    }

    /// Whether the regex matches anywhere in `text`.
    pub fn is_match(&self, text: &str) -> Result<bool, String> {
        let mut budget = BUDGET;
        let mut step = || budget.checked_sub(1).map(|left| budget = left).ok_or("regex work budget exhausted");
        let look = LookMatcher::new();
        let mut seen = vec![0; self.nfa.states().len()];
        let (mut seeds, mut active) = (Vec::<StateID>::new(), Vec::new());
        for at in 0..=text.len() {
            if text.is_char_boundary(at) { seeds.push(self.nfa.start_anchored()); }
            while let Some(id) = seeds.pop() {
                if seen[id.as_usize()] == at + 1 { continue; }
                seen[id.as_usize()] = at + 1;
                step()?;
                match self.nfa.state(id) {
                    State::ByteRange { .. } | State::Sparse(_) | State::Dense(_) => active.push(id),
                    State::Look { look: kind, next } => if look.matches(*kind, text.as_bytes(), at) { seeds.push(*next) },
                    State::Union { alternates } => seeds.extend(alternates.iter().copied()),
                    State::BinaryUnion { alt1, alt2 } => { seeds.push(*alt1); seeds.push(*alt2); }
                    State::Capture { next, slot, .. } => {
                        let holds = match self.groups.iter().find(|group| group.slot == slot.as_usize()) {
                            Some(group) => group.holds(text, at) == group.positive,
                            None => true,
                        };
                        if holds { seeds.push(*next); }
                    }
                    State::Match { .. } => return Ok(true),
                    State::Fail => {}
                }
            }
            if at == text.len() { break; }
            for id in active.drain(..) {
                step()?;
                let next = match self.nfa.state(id) {
                    State::ByteRange { trans } => trans.matches(text.as_bytes(), at).then_some(trans.next),
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

impl Group {
    fn holds(&self, text: &str, at: usize) -> bool {
        if self.behind {
            self.check.is_match(&text[..at])
        } else {
            self.check.is_match(Input::new(text).range(at..).anchored(Anchored::Yes))
        }
    }
}

/// A leading inline flags group (`(?i)`), which holds for the whole regex.
fn leading_flags(pattern: &str) -> &str {
    let Some(rest) = pattern.strip_prefix("(?") else { return "" };
    let Some(end) = rest.find(')') else { return "" };
    if rest[..end].chars().all(|c| c.is_ascii_alphabetic() || c == '-') && end > 0 { &pattern[..end + 3] } else { "" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test(text: &str, pattern: &str) -> bool {
        Lookaround::compile(pattern).unwrap().is_match(text).unwrap()
    }

    #[test]
    fn lookarounds_check_their_position_and_keep_alternatives() {
        for value in ["foo", "foobar", "bar", "barista", ""] {
            assert!(!test(value, "^(?!foo)(?!bar).+$"), "{value}");
        }
        assert!(test("other", "^(?!foo)(?!bar).+$"));
        assert!(!test("foo/bar", "foo/(?!bar)"));
        assert!(test("foo/baz", "foo/(?!bar)"));
        assert!(!test("@app/api/enums", "@app/(?!(api/enums$)).*"));
        assert!(test("@app/api/enums/foo", "@app/(?!(api/enums$)).*"));
        assert!(test("ab", "(?:a(?!b)|ab)"));
        assert!(!test("ac", "a(?=b)b"));
        assert!(test("xb", "(?<=x)b"));
        assert!(!test("yb", "(?<=x)b"));
        assert!(test("yb", "(?<!x)b"));
        assert!(test("", "(?!)|(?=)"));
        assert!(test("AB", "(?i)a(?=b)"));
        assert!(test("a", "(?=x){0}a"));
    }

    #[test]
    fn nested_or_unterminated_groups_are_errors() {
        for pattern in ["(?=(?!a))", "(?!a", "(?=a\\"] {
            assert!(Lookaround::compile(pattern).is_err(), "{pattern}");
        }
        assert!(has_lookaround("a(?=b)") && !has_lookaround("[(?=]") && !has_lookaround("\\(?=b"));
    }
}
