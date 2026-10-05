//! Parses one file into layer nodes.
//!
//! Three engines cooperate:
//! - pure-regex matchers run as DFAs ([`crate::pattern::dfa`])
//! - everything else runs on a memoised backtracker ("bit-state"): every
//!   (instruction, position) pair is explored at most once per run, and
//!   states that failed once stay dead for the rest of the file
//! - calls into other matchers are memoised per (matcher, position) like a
//!   packrat parser, so a string or bracket group is scanned once no matter
//!   how many layers look at it

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use regex_automata::meta::Cache;

use super::states::{Dead, Visited};
use super::text::{decode, look, utf8_len};
use super::{DfaCaches, Grammar, View};
use crate::pattern::compile::{Inst, Program, NO_LABEL};
use crate::pattern::dfa::Dfa;
use crate::pattern::prefilter::ByteSet;

/// `Node::matcher` of the file's root node.
pub const ROOT: u32 = u32::MAX;
/// `Node::matcher` of plain text captures (`name:/\w+/`).
pub const TEXT: u32 = u32::MAX - 1;

const FAIL: u32 = u32::MAX;
const BUSY: u32 = u32::MAX - 1;
/// Deeper nesting than this is reported, not followed (protects the stack).
const MAX_DEPTH: u32 = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub label: u16,
    pub node: u32,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub matcher: u32,
    pub start: u32,
    pub end: u32,
    pub children: Vec<Edge>,
}

#[derive(Clone, Copy)]
enum Event {
    Save { slot: u16, pos: u32 },
    Child { label: u16, node: u32 },
    Hole { close: bool, pos: u32 },
}

#[derive(Clone, Copy)]
struct Job {
    pc: u32,
    pos: u32,
    log: u32,
}

#[derive(Default)]
struct FxHasher(u64);

impl Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }

    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

type Memo = HashMap<u64, u32, BuildHasherDefault<FxHasher>>;

/// Buffers of one (possibly nested) program run, pooled across runs so the
/// hot path never allocates.
#[derive(Default)]
struct Frame {
    stack: Vec<Job>,
    log: Vec<Event>,
    visited: Visited,
}

pub struct Parser<'g, 't> {
    g: &'g Grammar,
    /// Overlays to search (by matcher); None searches all of them.
    pub enabled_overlays: Option<&'t [bool]>,
    /// `zlint --profile` counters, when profiling.
    pub profile: Option<&'t crate::engine::Profile>,
    view: &'g View,
    text: &'t [u8],
    memo: Memo,
    pub nodes: Vec<Node>,
    /// `(node, start, end)` of each node's inside (from its first `*` to the
    /// end of its last one), in node order.
    pub holes: Vec<(u32, u32, u32)>,
    free_pages: Vec<Box<[u64]>>,
    frames: Vec<Frame>,
    /// Per matcher; see [`Dead`].
    dead: Vec<Option<Dead>>,
    /// Borrowed from the grammar's pool for the parser's lifetime.
    caches: DfaCaches,
    /// Lookaround DFA caches, by LookDfa::id.
    look_caches: HashMap<u32, regex_automata::hybrid::dfa::Cache>,
    depth: u32,
    /// Set when nesting exceeded [`MAX_DEPTH`] and some structure was skipped.
    pub too_deep: bool,
    /// Per named layer: its top nodes found so far.
    layered: Vec<Vec<u32>>,
    /// `(root node, layer)` of each named layer's tree.
    pub layer_roots: Vec<(u32, u16)>,
}

/// Cheap checks before running a matcher at `pos`: its first byte, first two
/// bytes, and the byte before (from a leading lookbehind).
#[inline]
fn may_start(m: &super::Matcher, text: &[u8], pos: usize) -> bool {
    m.first.contains(text[pos]) && m.pairs.as_ref().is_none_or(|p| p.allows(text, pos)) && m.prev.as_ref().is_none_or(|p| p.allows(text, pos))
}

fn cache<'c>(slot: &'c mut Option<Cache>, dfa: &Dfa) -> &'c mut Cache {
    slot.get_or_insert_with(|| dfa.cache())
}

impl Drop for Parser<'_, '_> {
    fn drop(&mut self) {
        self.g.give_caches(std::mem::take(&mut self.caches));
    }
}

impl<'g, 't> Parser<'g, 't> {
    pub fn new(g: &'g Grammar, lang: usize, text: &'t str) -> Self {
        let n = g.matchers.len();
        Parser {
            g,
            enabled_overlays: None,
            profile: None,
            view: g.view(lang),
            text: text.as_bytes(),
            memo: Memo::default(),
            nodes: Vec::new(),
            holes: Vec::new(),
            free_pages: Vec::new(),
            frames: Vec::new(),
            dead: (0..n).map(|_| None).collect(),
            caches: g.take_caches(),
            look_caches: HashMap::new(),
            depth: 0,
            too_deep: false,
            layered: vec![Vec::new(); g.layers.len()],
            layer_roots: Vec::new(),
        }
    }

    /// Each named layer with nodes: a root node over the whole file holding
    /// its top nodes, under `under` (the file).
    fn root_layers(&mut self, under: u32) {
        for l in 0..self.layered.len() {
            let tops = std::mem::take(&mut self.layered[l]);
            if tops.is_empty() {
                continue;
            }
            let children = tops.into_iter().map(|n| Edge { label: NO_LABEL, node: n }).collect();
            let root = self.push(Node { matcher: ROOT, start: 0, end: self.text.len() as u32, children });
            self.nodes[under as usize].children.push(Edge { label: NO_LABEL, node: root });
            self.layer_roots.push((root, l as u16));
        }
    }

    /// Scans the whole file as the language's root layer, then searches
    /// overlays. Returns the root node.
    pub fn parse(&mut self) -> u32 {
        let edges = self.root_edges();
        let root = self.push(Node { matcher: ROOT, start: 0, end: self.text.len() as u32, children: edges });
        self.overlays(root);
        self.root_layers(root);
        root
    }

    /// The language's root layers across the whole file.
    fn root_edges(&mut self) -> Vec<Edge> {
        let view = self.view;
        let text = self.text;
        let mut edges = Vec::new();
        let mut pos = 0;
        while let Some(p) = view.root_prefilter.next(text, pos) {
            match self.longest(&view.root_children, p, text.len()) {
                Some(id) => {
                    edges.push(Edge { label: NO_LABEL, node: id });
                    pos = (self.nodes[id as usize].end as usize).max(p + 1);
                }
                None => pos = p + utf8_len(text[p]),
            }
        }
        edges
    }

    /// Pattern layers in a front end's tree (`nodes`, already in this
    /// grammar's ids), by the rules pattern trees follow: a node's child
    /// layers fill the gaps between its other children, and its overlays
    /// search its whole text. The
    /// pattern nodes are appended to `nodes`; returns their holes (see
    /// [`Parser::holes`]) with the new ids.
    pub fn graft(&mut self, nodes: &mut Vec<Node>, root: u32) -> Vec<(u32, u32, u32)> {
        let view = self.view;
        let spans = |id: u32| -> Vec<(usize, usize)> {
            nodes[id as usize].children.iter().map(|e| &nodes[e.node as usize]).map(|c| (c.start as usize, c.end as usize)).collect()
        };
        // Placeholders stand in for the native hosts while matching.
        let mut hosts: Vec<(u32, u32)> = Vec::new();
        // The language's own layers: across the whole file, under the
        // program (until no std layer lives there, the regex JS grammar
        // still does).
        let edges = self.root_edges();
        let root_host = self.push(Node { matcher: ROOT, start: 0, end: self.text.len() as u32, children: edges });
        hosts.push((root, root_host));
        for (id, n) in nodes.iter().enumerate() {
            let m = n.matcher;
            if m == TEXT || m == ROOT || (view.children[m as usize].is_empty() && view.overlays[m as usize].is_empty()) {
                continue;
            }
            let host = self.push(Node { matcher: m, start: n.start, end: n.end, children: Vec::new() });
            if !view.children[m as usize].is_empty() {
                self.fill_gaps(host, &view.children[m as usize], &view.child_first[m as usize], spans(id as u32));
            }
            hosts.push((id as u32, host));
        }
        // One overlay pass over every host (an empty span hosts nothing
        // itself), so nodes several hosts share are searched once.
        let all = self.push(Node { matcher: ROOT, start: 0, end: 0, children: hosts.iter().map(|&(_, h)| Edge { label: NO_LABEL, node: h }).collect() });
        self.overlays(all);
        self.root_layers(root_host);

        let offset = nodes.len() as u32;
        for r in &mut self.layer_roots {
            r.0 += offset;
        }
        let mut placeholder = vec![None; self.nodes.len()];
        for &(native, host) in &hosts {
            placeholder[host as usize] = Some(native);
        }
        let mut moved = Vec::new();
        for (i, mut n) in std::mem::take(&mut self.nodes).into_iter().enumerate() {
            for e in &mut n.children {
                e.node += offset;
            }
            if let Some(native) = placeholder[i] {
                moved.push((native, std::mem::take(&mut n.children)));
            }
            // Placeholders stay as inert text so ids keep their places.
            if placeholder[i].is_some() || i as u32 == all {
                n = Node { matcher: TEXT, start: n.start, end: n.start, children: Vec::new() };
            }
            nodes.push(n);
        }
        for (native, edges) in moved {
            nodes[native as usize].children.extend(edges);
        }
        std::mem::take(&mut self.holes).into_iter().map(|(n, s, e)| (n + offset, s, e)).collect()
    }

    fn push(&mut self, node: Node) -> u32 {
        self.nodes.push(node);
        self.nodes.len() as u32 - 1
    }

    fn leaf(&mut self, m: u32, start: usize, end: usize) -> u32 {
        let id = self.push(Node { matcher: m, start: start as u32, end: end as u32, children: Vec::new() });
        self.fill_children(id);
        id
    }

    /// A matcher without `*` has nowhere in its pattern for children to
    /// appear, so its children are searched across its text afterwards
    /// (skipping its fields), the same way a language root is.
    fn fill_children(&mut self, id: u32) {
        let g = self.g;
        let view = self.view;
        let m = self.nodes[id as usize].matcher;
        let list = &view.children[m as usize];
        if list.is_empty() || g.matchers[m as usize].has_skip {
            return;
        }
        if self.depth >= MAX_DEPTH {
            self.too_deep = true;
            return;
        }
        let mut covered = Vec::new();
        self.structural_children(id, &mut covered);
        self.fill_gaps(id, list, &view.child_first[m as usize], covered);
    }

    /// Children from `list` in the gaps of `id`'s text that `covered`
    /// leaves: children never start inside another child.
    fn fill_gaps(&mut self, id: u32, list: &[u32], cf: &ByteSet, mut covered: Vec<(usize, usize)>) {
        let text = self.text;
        let (m, start, end) = {
            let n = &self.nodes[id as usize];
            (n.matcher, n.start as usize, n.end as usize)
        };
        covered.sort_unstable();
        covered.push((end, end));
        self.depth += 1;
        let mut found = Vec::new();
        let mut pos = start;
        for (gap_end, next) in covered {
            while pos < gap_end {
                if cf.contains(text[pos]) {
                    if let Some(n) = self.longest(list, pos, gap_end).filter(|&n| !(self.nodes[n as usize].matcher == m && pos == start)) {
                        found.push(Edge { label: NO_LABEL, node: n });
                        pos = (self.nodes[n as usize].end as usize).max(pos + 1);
                        continue;
                    }
                }
                pos += utf8_len(text[pos]);
            }
            pos = pos.max(next);
        }
        self.depth -= 1;
        self.nodes[id as usize].children.extend(found);
    }

    /// Ranges owned by child matchers (looking through text captures).
    fn structural_children(&self, id: u32, out: &mut Vec<(usize, usize)>) {
        for e in &self.nodes[id as usize].children {
            let n = &self.nodes[e.node as usize];
            if n.matcher == TEXT {
                self.structural_children(e.node, out);
            } else {
                out.push((n.start as usize, n.end as usize));
            }
        }
    }

    fn hosts_overlay(&self, node: u32, o: u32) -> bool {
        match self.nodes[node as usize].matcher {
            ROOT => self.view.root_overlays.contains(&o),
            TEXT => false,
            m => self.view.overlays[m as usize].contains(&o),
        }
    }

    /// Ranges of the nearest descendants of `id` that host overlay `o`
    /// themselves, sorted. `id` leaves those to them, so every position is
    /// searched once, by its innermost host.
    fn overlay_holes(&self, id: u32, o: u32) -> Vec<(usize, usize)> {
        let mut holes = Vec::new();
        let mut stack: Vec<u32> = self.nodes[id as usize].children.iter().map(|e| e.node).collect();
        while let Some(c) = stack.pop() {
            let n = &self.nodes[c as usize];
            if self.hosts_overlay(c, o) {
                holes.push((n.start as usize, n.end as usize));
            } else {
                stack.extend(n.children.iter().map(|e| e.node));
            }
        }
        holes.sort_unstable();
        holes
    }

    /// Searches overlay matchers inside every node that has them. Overlay
    /// searches are never repeated, so they skip the memo.
    fn overlays(&mut self, root: u32) {
        let g = self.g;
        let view = self.view;
        let text = self.text;
        let mut seen = vec![false; self.nodes.len()];
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if seen.len() <= id as usize {
                seen.resize(self.nodes.len().max(id as usize + 1), false);
            }
            if std::mem::replace(&mut seen[id as usize], true) {
                continue;
            }
            let (m, start, end) = {
                let n = &self.nodes[id as usize];
                (n.matcher, n.start as usize, n.end as usize)
            };
            let list: &[u32] = match m {
                ROOT => &view.root_overlays,
                TEXT => &[],
                m => &view.overlays[m as usize],
            };
            for &o in list {
                if self.enabled_overlays.is_some_and(|e| !e[o as usize]) {
                    continue;
                }
                let om = &g.matchers[o as usize];
                let started = self.profile.map(|_| (std::time::Instant::now(), self.nodes.len()));
                let holes = self.overlay_holes(id, o);
                let mut hole = 0;
                let mut pos = start;
                while pos < end {
                    let found = match (&om.dfa, &om.guard) {
                        (Some(dfa), _) => dfa.find(cache(&mut self.caches.dfa[o as usize], dfa), &text[..end], pos),
                        // The guard matches wherever the matcher does: its
                        // leftmost match is the next place worth trying.
                        (None, Some(guard)) => guard.find(cache(&mut self.caches.guard[o as usize], guard), &text[..end], pos),
                        (None, None) => om.prefilter.next(&text[..end], pos).map(|p| (p, p)),
                    };
                    let Some((s, e)) = found else { break };
                    // Matches may run into a nested host, but not start there.
                    while hole < holes.len() && holes[hole].1 <= s {
                        hole += 1;
                    }
                    if hole < holes.len() && holes[hole].0 <= s {
                        pos = holes[hole].1;
                        continue;
                    }
                    let node = match &om.dfa {
                        Some(_) if !om.has_captures => Some(self.leaf(o, s, e)),
                        _ if !may_start(om, text, s) => None,
                        _ => self.match_at(o, s, end),
                    };
                    match node {
                        Some(n) => {
                            // A named layer's nodes go in that layer's tree.
                            match om.layer {
                                Some(l) => {
                                    self.layered[l as usize].push(n);
                                    stack.push(n);
                                }
                                None => self.nodes[id as usize].children.push(Edge { label: NO_LABEL, node: n }),
                            }
                            pos = (self.nodes[n as usize].end as usize).max(s + 1);
                        }
                        None => pos = s + utf8_len(text[s]),
                    }
                }
                if let (Some(p), Some((t, nodes))) = (self.profile, started) {
                    crate::engine::Profile::add_time(&p.overlay_nanos[o as usize], t.elapsed());
                    crate::engine::Profile::add(&p.overlay_nodes[o as usize], (self.nodes.len() - nodes) as u64);
                }
            }
            stack.extend(self.nodes[id as usize].children.iter().map(|e| e.node));
        }
    }

    /// The longest-matching child at `pos`; ties go to the earliest declared.
    fn longest(&mut self, list: &[u32], pos: usize, limit: usize) -> Option<u32> {
        let b = self.text[pos];
        let mut best: Option<u32> = None;
        for &c in list {
            if !self.g.matchers[c as usize].first.contains(b) {
                continue;
            }
            if let Some(n) = self.call(c, pos) {
                let end = self.nodes[n as usize].end;
                // Longer wins; on a tie a specialisation beats its base.
                let better = best.is_none_or(|b| {
                    let other = &self.nodes[b as usize];
                    end > other.end || (end == other.end && c != other.matcher && self.g.is_a(c, other.matcher))
                });
                if end as usize <= limit && better {
                    best = Some(n);
                }
            }
        }
        best
    }

    /// Matches matcher `m` at `pos`.
    pub fn call(&mut self, m: u32, pos: usize) -> Option<u32> {
        let g = self.g;
        let text = self.text;
        let matcher = &g.matchers[m as usize];
        if pos >= text.len() || !may_start(matcher, text, pos) {
            return None;
        }
        // DFAs answer most calls without the interpreter or the memo.
        if let Some(dfa) = &matcher.dfa {
            let end = dfa.anchored(cache(&mut self.caches.dfa[m as usize], dfa), text, pos)?;
            if !matcher.has_captures {
                return Some(self.leaf(m, pos, end));
            }
        } else if let Some(guard) = &matcher.guard {
            guard.anchored(cache(&mut self.caches.guard[m as usize], guard), text, pos)?;
        }
        let key = ((m as u64) << 32) | pos as u64;
        match self.memo.get(&key) {
            Some(&FAIL) | Some(&BUSY) => return None,
            Some(&n) => return Some(n),
            None => {}
        }
        if self.depth >= MAX_DEPTH {
            self.too_deep = true;
            return None;
        }
        self.memo.insert(key, BUSY);
        let id = self.match_at(m, pos, text.len());
        self.memo.insert(key, id.unwrap_or(FAIL));
        id
    }

    /// Runs matcher `m`'s program at `pos` (no memo) and builds its node.
    fn match_at(&mut self, m: u32, pos: usize, limit: usize) -> Option<u32> {
        let g = self.g;
        if !g.matchers[m as usize].first.contains(self.text[pos]) {
            return None;
        }
        self.depth += 1;
        let result = self.run(&g.matchers[m as usize].prog, Some(m), pos, limit, None);
        self.depth -= 1;
        let (end, frame) = result?;
        let id = self.build_node(m, pos, end, &frame.log);
        self.give(frame);
        self.fill_children(id);
        Some(id)
    }

    fn take(&mut self, width: usize, start: usize) -> Frame {
        let mut f = self.frames.pop().unwrap_or_default();
        f.stack.clear();
        f.log.clear();
        f.visited.reset(width, start);
        f
    }

    fn give(&mut self, mut f: Frame) {
        f.visited.recycle(&mut self.free_pages);
        self.frames.push(f);
    }

    fn build_node(&mut self, m: u32, start: usize, end: usize, log: &[Event]) -> u32 {
        let mut open: Vec<(u16, u32, Vec<Edge>)> = Vec::new();
        let mut cur: Vec<Edge> = Vec::new();
        let mut hole: Option<(u32, u32)> = None;
        for &ev in log {
            match ev {
                Event::Child { label, node } => cur.push(Edge { label, node }),
                Event::Save { slot, pos } if slot % 2 == 0 => open.push((slot / 2, pos, std::mem::take(&mut cur))),
                Event::Save { pos, .. } => {
                    if let Some((label, s, parent)) = open.pop() {
                        let inner = std::mem::replace(&mut cur, parent);
                        let id = self.push(Node { matcher: TEXT, start: s, end: pos, children: inner });
                        cur.push(Edge { label, node: id });
                    }
                }
                Event::Hole { close: false, pos } => hole = Some(hole.map_or((pos, pos), |(s, _)| (s, pos))),
                Event::Hole { close: true, pos } => hole = hole.map(|(s, _)| (s, pos)),
            }
        }
        let id = self.push(Node { matcher: m, start: start as u32, end: end as u32, children: cur });
        if let Some((s, e)) = hole {
            self.holes.push((id, s, e));
        }
        id
    }

    /// Runs `prog` from `start`. `owner` is the matcher whose children `*`
    /// may consume. With `must_end`, only a match ending exactly there counts.
    fn run(&mut self, prog: &Program, owner: Option<u32>, start: usize, limit: usize, must_end: Option<usize>) -> Option<(usize, Frame)> {
        let text = self.text;
        let view = self.view;
        // Only full-text runs of a matcher are pure functions of position.
        let pure = owner.filter(|_| limit == text.len() && must_end.is_none());
        let child_first = owner.map(|o| &view.child_first[o as usize]);
        let mut frame = self.take(prog.insts.len(), start);
        let Frame { stack, log, visited } = &mut frame;
        stack.push(Job { pc: 0, pos: start as u32, log: 0 });
        let mut steps = 0u64;

        let found = 'search: loop {
            let Some(job) = stack.pop() else { break None };
            log.truncate(job.log as usize);
            let (mut pc, mut pos) = (job.pc as usize, job.pos as usize);
            loop {
                steps += 1;
                if !visited.insert(pc, pos, &mut self.free_pages) {
                    break;
                }
                if let Some(Some(dead)) = pure.map(|m| &self.dead[m as usize]) {
                    if dead.contains(pc, pos) {
                        break;
                    }
                }
                match prog.insts[pc] {
                    Inst::Lit(i) => {
                        let lit = &prog.lits[i as usize];
                        if pos + lit.len() > limit || text[pos..pos + lit.len()] != lit[..] {
                            break;
                        }
                        pos += lit.len();
                    }
                    Inst::LitFold(i) => {
                        let lit = &prog.lits[i as usize];
                        if pos + lit.len() > limit || !text[pos..pos + lit.len()].eq_ignore_ascii_case(lit) {
                            break;
                        }
                        pos += lit.len();
                    }
                    Inst::Class(i) => {
                        if pos >= limit {
                            break;
                        }
                        let (c, len) = decode(text, pos);
                        if !prog.classes[i as usize].contains(c) {
                            break;
                        }
                        pos += len;
                    }
                    Inst::Any => {
                        if pos >= limit {
                            break;
                        }
                        pos += utf8_len(text[pos]);
                    }
                    Inst::AnyNoNl => {
                        if pos >= limit || text[pos] == b'\n' {
                            break;
                        }
                        pos += utf8_len(text[pos]);
                    }
                    Inst::Split(a, b) => {
                        stack.push(Job { pc: b, pos: pos as u32, log: log.len() as u32 });
                        pc = a as usize;
                        continue;
                    }
                    Inst::Jmp(t) => {
                        pc = t as usize;
                        continue;
                    }
                    Inst::Save(slot) => log.push(Event::Save { slot, pos: pos as u32 }),
                    Inst::Hole(close) => log.push(Event::Hole { close, pos: pos as u32 }),
                    Inst::Look(l) => {
                        if !look(text, pos, limit, l) {
                            break;
                        }
                    }
                    Inst::Around(i) => {
                        let around = &prog.arounds[i as usize];
                        let cache = self.look_caches.entry(around.id).or_insert_with(|| around.cache());
                        if !around.holds(cache, &text[..limit], pos) {
                            break;
                        }
                    }
                    Inst::Call { matcher, label } => match self.call(matcher, pos) {
                        Some(n) if self.nodes[n as usize].end as usize <= limit => {
                            log.push(Event::Child { label, node: n });
                            pos = self.nodes[n as usize].end as usize;
                        }
                        _ => break,
                    },
                    Inst::Skip(i) => {
                        if pos >= limit {
                            break;
                        }
                        let b = text[pos];
                        let child = match (owner, child_first) {
                            (Some(o), Some(cf)) if cf.contains(b) => self.longest(&view.children[o as usize], pos, limit),
                            _ => None,
                        };
                        match child {
                            Some(n) => {
                                log.push(Event::Child { label: NO_LABEL, node: n });
                                pos = self.nodes[n as usize].end as usize;
                            }
                            None => pos += utf8_len(b),
                        }
                        // Positions where neither a child nor what follows
                        // the `*` can start would just be stepped over.
                        if let Some(Some(stop)) = prog.skip_stops.get(i as usize) {
                            while pos < limit {
                                let b = text[pos];
                                if stop.contains(b) || child_first.is_some_and(|cf| cf.contains(b)) {
                                    break;
                                }
                                pos += 1;
                            }
                        }
                    }
                    Inst::Match => {
                        // Layers never match empty text: keep backtracking
                        // for a longer way through the pattern.
                        let empty = owner.is_some() && pos == start;
                        if !empty && must_end.is_none_or(|e| e == pos) {
                            break 'search Some(pos);
                        }
                        break;
                    }
                }
                pc += 1;
            }
        };
        if let (Some(p), Some(m)) = (self.profile, owner) {
            crate::engine::Profile::add(&p.matcher_steps[m as usize], steps);
            crate::engine::Profile::add(&p.matcher_runs[m as usize], 1);
        }
        match found {
            Some(end) => Some((end, frame)),
            None => {
                if let Some(m) = pure {
                    let len = text.len();
                    self.dead[m as usize].get_or_insert_with(|| Dead::new(prog.insts.len(), len)).absorb(&frame.visited);
                }
                self.give(frame);
                None
            }
        }
    }
}

/// What a candidate child matcher does at one position (`zlint tree --at`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// Its first-byte set rules the byte out.
    FirstByte,
    /// Its two-byte or lookbehind prefilter rules the position out.
    Prefilter,
    NoMatch,
    /// Matches, ending at this offset.
    Match(usize),
    /// Matches, but runs past the end of the layer it would sit in.
    TooLong(usize),
}

/// Why the layer tree looks the way it does at `pos`: whether the scan of
/// the containing layer (`parent`, `None` for the file root) even stops
/// there, and what each of its child matchers would do. Overlays are
/// searched separately and are not included.
pub fn probe(g: &Grammar, lang: usize, text: &str, parent: Option<u32>, pos: usize, limit: usize) -> (bool, Vec<(u32, Probe)>) {
    let mut p = Parser::new(g, lang, text);
    let view = p.view;
    let bytes = text.as_bytes();
    if pos >= bytes.len() {
        return (false, Vec::new());
    }
    let (list, scanned) = match parent {
        None => (&view.root_children, view.root_prefilter.next(bytes, pos) == Some(pos)),
        Some(m) => (&view.children[m as usize], view.child_first[m as usize].contains(bytes[pos])),
    };
    let out = list
        .iter()
        .map(|&c| {
            let m = &g.matchers[c as usize];
            let outcome = if !m.first.contains(bytes[pos]) {
                Probe::FirstByte
            } else if !may_start(m, bytes, pos) {
                Probe::Prefilter
            } else {
                match p.call(c, pos) {
                    Some(n) => {
                        let end = p.nodes[n as usize].end as usize;
                        if end <= limit {
                            Probe::Match(end)
                        } else {
                            Probe::TooLong(end)
                        }
                    }
                    None => Probe::NoMatch,
                }
            };
            (c, outcome)
        })
        .collect();
    (scanned, out)
}

/// Does `prog` match all of `text`? Powers `~` when no DFA can.
pub fn full_match(g: &Grammar, lang: usize, prog: &Program, text: &str) -> bool {
    let mut p = Parser::new(g, lang, text);
    let len = text.len();
    p.run(prog, None, 0, len, Some(len)).is_some()
}
