//! Code compared the way eslint's `equalTokens` compares it: the same
//! tokens, whatever the spacing and comments between them. And the rules
//! built on that comparison: duplicate `else if` conditions, duplicate
//! `case`s, and `for` loops counting the wrong way.

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};

impl NativeFile<'_> {
    fn unparen(&self, node: u32) -> u32 {
        self.syntax.unparen(node)
    }

    /// eslint's `equalTokens` (outer parentheses aside).
    pub fn equal_tokens(&self, a: u32, b: u32) -> bool {
        self.tokens(self.unparen(a)) == self.tokens(self.unparen(b))
    }

    /// no-dupe-else-if's `equal`: `&&` / `||` in either order, else tokens.
    fn equal_condition(&self, a: u32, b: u32) -> bool {
        let s = &self.syntax;
        let (a, b) = (self.unparen(a), self.unparen(b));
        if s.kind(a) != s.kind(b) {
            return false;
        }
        if s.is(a, K::Logical) && matches!(s.word(a, F::Op), Some("&&" | "||")) && s.word(a, F::Op) == s.word(b, F::Op) {
            let (Some(al), Some(ar), Some(bl), Some(br)) = (s.get(a, F::Left), s.get(a, F::Right), s.get(b, F::Left), s.get(b, F::Right)) else { return false };
            return self.equal_condition(al, bl) && self.equal_condition(ar, br) || self.equal_condition(al, br) && self.equal_condition(ar, bl);
        }
        self.equal_tokens(a, b)
    }

    /// The operands of a chain of one logical operator (`a || (b || c)`:
    /// `a`, `b`, `c`).
    fn split_by(&self, node: u32, op: &str) -> Vec<u32> {
        let s = &self.syntax;
        let node = self.unparen(node);
        if s.is(node, K::Logical) && s.word(node, F::Op) == Some(op) {
            let mut out = Vec::new();
            for f in [F::Left, F::Right] {
                if let Some(x) = s.get(node, f) {
                    out.extend(self.split_by(x, op));
                }
            }
            return out;
        }
        vec![node]
    }

    /// no-dupe-else-if on an `if` whose test an earlier branch of its chain
    /// already covers.
    pub fn duplicate_else_if(&self, if_node: u32) -> bool {
        let s = &self.syntax;
        let Some(test) = s.get(if_node, F::Test) else { return false };
        let bare = self.unparen(test);
        let mut conditions = vec![bare];
        if s.is(bare, K::Logical) && s.word(bare, F::Op) == Some("&&") {
            conditions.extend(self.split_by(bare, "&&"));
        }
        let mut lists: Vec<Vec<Vec<u32>>> = conditions.iter().map(|&c| self.split_by(c, "||").into_iter().map(|o| self.split_by(o, "&&")).collect()).collect();
        let mut current = if_node;
        while let Some(parent) = s.parent(current).filter(|&p| s.is(p, K::If) && s.get(p, F::Alternate) == Some(current)) {
            current = parent;
            let Some(ptest) = s.get(parent, F::Test) else { break };
            let earlier: Vec<Vec<u32>> = self.split_by(ptest, "||").into_iter().map(|o| self.split_by(o, "&&")).collect();
            // `a && b` is covered by an earlier `a`: every part of the
            // earlier operand is in it.
            let subset = |small: &[u32], big: &[u32]| small.iter().all(|&x| big.iter().any(|&y| self.equal_condition(x, y)));
            for ors in &mut lists {
                ors.retain(|operand| !earlier.iter().any(|e| subset(e, operand)));
            }
            if lists.iter().any(Vec::is_empty) {
                return true;
            }
        }
        false
    }

    /// no-self-assign on an assignment: the right-hand nodes assigned to
    /// themselves, each with whether it is a member (`a.b = a.b`).
    pub fn self_assignments(&self, assign: u32) -> Vec<(u32, bool)> {
        let s = &self.syntax;
        let mut out = Vec::new();
        if s.is(assign, K::Assign) && matches!(s.word(assign, F::Op), Some("=" | "&&=" | "||=" | "??=")) {
            if let (Some(l), Some(r)) = (s.get(assign, F::Left), s.get(assign, F::Right)) {
                self.each_self_assignment(l, r, &mut out);
            }
        }
        out
    }

    /// eslint's `eachSelfAssignment`, over destructuring written as array
    /// and object literals.
    fn each_self_assignment(&self, left: u32, right: u32, out: &mut Vec<(u32, bool)>) {
        let s = &self.syntax;
        let (left, right) = (self.unparen(left), self.unparen(right));
        match (s.kind(left), s.kind(right)) {
            (Some(K::Ident), Some(K::Ident)) if s.text(left) == s.text(right) => out.push((right, false)),
            (Some(K::Array), Some(K::Array)) => {
                let (ls, rs): (Vec<u32>, Vec<u32>) = (s.all(left, F::Element).collect(), s.all(right, F::Element).collect());
                for i in 0..ls.len().min(rs.len()) {
                    // `[...a] = [...a, 1]`
                    if s.is(ls[i], K::Spread) && i + 1 < rs.len() {
                        break;
                    }
                    self.each_self_assignment(ls[i], rs[i], out);
                    if s.is(rs[i], K::Spread) {
                        break;
                    }
                }
            }
            (Some(K::Spread), Some(K::Spread)) => {
                if let (Some(a), Some(b)) = (s.get(left, F::Arg), s.get(right, F::Arg)) {
                    self.each_self_assignment(a, b, out);
                }
            }
            (Some(K::Object), Some(K::Object)) => {
                let rs: Vec<u32> = s.all(right, F::Property).collect();
                if rs.is_empty() {
                    return;
                }
                // Properties after the last spread may overwrite.
                let start = rs.iter().rposition(|&p| s.is(p, K::Spread)).map_or(0, |i| i + 1);
                for l in s.all(left, F::Property).collect::<Vec<_>>() {
                    for &r in &rs[start..] {
                        self.each_self_assignment(l, r, out);
                    }
                }
            }
            (Some(K::Prop), Some(K::Prop)) => {
                let name = super::statics::property_name(s, left);
                if name.is_some() && name == super::statics::property_name(s, right) {
                    let value = |p: u32| s.get(p, F::Value).or_else(|| s.get(p, F::Key));
                    if let (Some(a), Some(b)) = (value(left), value(right)) {
                        self.each_self_assignment(a, b, out);
                    }
                }
            }
            (Some(K::Member | K::Index), Some(K::Member | K::Index)) if self.same_reference(left, right) => out.push((right, true)),
            _ => {}
        }
    }

    /// eslint's `isSameReference`.
    pub(crate) fn same_reference(&self, a: u32, b: u32) -> bool {
        self.same_reference_keyed(a, b, false)
    }

    /// eslint's `isSameReference(a, b, disableStaticComputedKey)`: with
    /// `literal_keys_differ`, `x.y` and `x["y"]` are different references.
    pub(crate) fn same_reference_keyed(&self, a: u32, b: u32, literal_keys_differ: bool) -> bool {
        let s = &self.syntax;
        let (a, b) = (self.unparen(a), self.unparen(b));
        match (s.kind(a), s.kind(b)) {
            (Some(K::Super), Some(K::Super)) | (Some(K::This), Some(K::This)) => true,
            (Some(K::Ident), Some(K::Ident)) | (Some(K::PrivateName), Some(K::PrivateName)) => s.text(a) == s.text(b),
            (Some(ka), Some(kb)) if ka == kb && matches!(ka, K::String | K::Number | K::Bool | K::Null | K::BigInt | K::Regex) => {
                super::statics::literal_value(s, a) == super::statics::literal_value(s, b)
            }
            (Some(ka @ (K::Member | K::Index)), Some(kb @ (K::Member | K::Index))) => {
                let same = |x: u32, y: u32| self.same_reference_keyed(x, y, literal_keys_differ);
                let same_object = s.get(a, F::Object).zip(s.get(b, F::Object)).is_some_and(|(x, y)| same(x, y));
                if !literal_keys_differ {
                    if let Some(name) = super::statics::property_name(s, a) {
                        return same_object && super::statics::property_name(s, b).as_deref() == Some(&name);
                    }
                }
                ka == kb && same_object && {
                    let key = |n: u32| s.get(n, F::Property).or_else(|| s.get(n, F::Index));
                    key(a).zip(key(b)).is_some_and(|(x, y)| same(x, y))
                }
            }
            _ => false,
        }
    }

    /// no-duplicate-case on a `case` whose test an earlier case of its
    /// `switch` has (by tokens).
    pub fn duplicate_case(&self, case: u32) -> bool {
        let s = &self.syntax;
        let (Some(test), Some(switch)) = (s.get(case, F::Test), s.parent(case)) else { return false };
        s.all(switch, F::Case).take_while(|&c| c != case).filter_map(|c| s.get(c, F::Test)).any(|t| self.equal_tokens(t, test))
    }

    /// for-direction: the update moves the counter away from where the
    /// test lets the loop end.
    pub fn for_wrong_direction(&self, for_node: u32) -> bool {
        let s = &self.syntax;
        let (Some(test), Some(update)) = (s.get(for_node, F::Test), s.get(for_node, F::Update)) else { return false };
        let test = self.unparen(test);
        if !s.is(test, K::Binary) {
            return false;
        }
        let op = s.word(test, F::Op).unwrap_or("");
        for (field, left) in [(F::Left, true), (F::Right, false)] {
            let Some(counter) = s.get(test, field).map(|c| self.unparen(c)).filter(|&c| s.is(c, K::Ident)) else { continue };
            let name = s.text(counter);
            let wrong = match op {
                "<" | "<=" => {
                    if left {
                        -1
                    } else {
                        1
                    }
                }
                ">" | ">=" => {
                    if left {
                        1
                    } else {
                        -1
                    }
                }
                _ => return false,
            };
            let mutating = self.counter_updates(update, name);
            if mutating.len() == 1 && self.update_direction(mutating[0]) == wrong {
                return true;
            }
        }
        false
    }

    /// The parts of a `for` update that change the counter.
    fn counter_updates(&self, node: u32, counter: &str) -> Vec<u32> {
        let s = &self.syntax;
        let node = self.unparen(node);
        match s.kind(node) {
            Some(K::Sequence) => s.all(node, F::Expr).flat_map(|e| self.counter_updates(e, counter)).collect(),
            Some(K::Update) if s.get(node, F::Arg).map(|a| self.unparen(a)).is_some_and(|a| s.is(a, K::Ident) && s.text(a) == counter) => vec![node],
            Some(K::Assign) if s.get(node, F::Left).map(|a| self.unparen(a)).is_some_and(|a| s.is(a, K::Ident) && s.text(a) == counter) => vec![node],
            _ => Vec::new(),
        }
    }

    fn update_direction(&self, node: u32) -> i32 {
        let s = &self.syntax;
        match s.kind(node) {
            Some(K::Update) => match s.word(node, F::Op) {
                Some("++") => 1,
                Some("--") => -1,
                _ => 0,
            },
            Some(K::Assign) => {
                let dir = match s.word(node, F::Op) {
                    Some("+=") => 1,
                    Some("-=") => -1,
                    _ => return 0,
                };
                match s.get(node, F::Right).and_then(|r| self.static_number(r)) {
                    Some(v) if v > 0.0 => dir,
                    Some(v) if v < 0.0 => -dir,
                    _ => 0,
                }
            }
            _ => 0,
        }
    }
}
