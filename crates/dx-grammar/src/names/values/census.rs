//! Keys for [`dx_core::work`]'s census: a fact by what it means in any linted
//! file, not this world's ids.

use std::any::Any;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};

use dx_core::fx_hash::{FxHashMap, FxHasher};
use crate::names::question::{FrameId, Leaf, NodeRef, NodeValue, Target};

use super::{Key, Values};

thread_local! {
    /// Per question context, frame and looseness: its key.
    static FRAMES: RefCell<FxHashMap<(usize, u32, bool), u64>> = RefCell::new(FxHashMap::default());
}

impl Values<'_> {
    fn census_node(&self, node: NodeRef, hash: &mut FxHasher) {
        if let Some(path) = self.world.path(node.file) { path.hash(hash) }
        node.node.hash(hash);
    }

    fn census_frame(&self, frame: FrameId, loose: bool, depth: u32) -> u64 {
        if frame == FrameId::ROOT { return 0 }
        if depth > 48 { return 1 }
        let context = std::ptr::from_ref(self.question) as usize;
        if let Some(found) = FRAMES.with(|frames| frames.borrow().get(&(context, frame.0, loose)).copied()) { return found }
        let mut hash = FxHasher::default();
        if let Some(binding) = self.question.get(frame) {
            self.census_frame(binding.parent, loose, depth + 1).hash(&mut hash);
            self.census_node(binding.owner, &mut hash);
            binding.classes.hash(&mut hash);
            if let Some((overlay, slots)) = binding.args.and_then(|args| self.question.arguments(args)) {
                overlay.hash(&mut hash);
                for slot in &slots {
                    match slot {
                        Some(value) => self.census_value(value, loose, depth + 1).hash(&mut hash),
                        None => 2u64.hash(&mut hash),
                    }
                }
            }
            match (binding.through, loose) {
                // What it binds names it: the route read adds nothing.
                (Some(_), true) if binding.args.is_some() => {}
                // Its arguments are this syntax, wherever it is written.
                (Some(through), true) => self.census_text(through).hash(&mut hash),
                (Some(through), false) => self.census_leaf_at(through, loose, depth + 1).hash(&mut hash),
                (None, _) => {}
            }
        }
        let key = hash.finish();
        FRAMES.with(|frames| frames.borrow_mut().insert((context, frame.0, loose), key));
        key
    }

    fn census_text(&self, leaf: Leaf) -> String {
        let Target::Node(node) = leaf.target else { return format!("{:?}", leaf.target) };
        let span = self.world.tree(node.file).and_then(|tree| tree.nodes.get(node.node as usize))
            .and_then(|data| self.world.text(node.file)?.get(data.start as usize..data.end as usize));
        span.unwrap_or("").to_owned()
    }

    fn census_leaf_at(&self, leaf: Leaf, loose: bool, depth: u32) -> u64 {
        let mut hash = FxHasher::default();
        match leaf.target {
            Target::Node(node) => { 1u8.hash(&mut hash); self.census_node(node, &mut hash) }
            Target::Scope(scope) => {
                2u8.hash(&mut hash);
                if let Some(path) = self.world.path(scope.file) { path.hash(&mut hash) }
                scope.scope.hash(&mut hash);
            }
            Target::Traits(bits) => { 3u8.hash(&mut hash); bits.hash(&mut hash) }
            Target::Key(id) => { 4u8.hash(&mut hash); self.census_key(self.question.key(id).as_ref()).hash(&mut hash) }
            Target::Product(id) => {
                5u8.hash(&mut hash);
                if let Some(product) = self.question.product(id) {
                    product.shape.0.hash(&mut hash);
                    (product.array, product.readonly).hash(&mut hash);
                    if depth < 48 {
                        for element in &product.elements {
                            self.census_value(&element.value, loose, depth + 1).hash(&mut hash);
                            (element.optional, element.repeated, element.values).hash(&mut hash);
                        }
                    }
                }
            }
            Target::Modified(id) => {
                6u8.hash(&mut hash);
                if let Some(modified) = self.question.modified(id) {
                    if depth < 48 { self.census_leaf_at(modified.source, loose, depth + 1).hash(&mut hash) }
                    modified.required.hash(&mut hash);
                    modified.flags.hash(&mut hash);
                }
            }
        }
        self.census_frame(leaf.frame, loose, depth + 1).hash(&mut hash);
        hash.finish()
    }

    fn census_value(&self, value: &NodeValue, loose: bool, depth: u32) -> u64 {
        let mut hash = FxHasher::default();
        match value {
            NodeValue::Alternatives(rows) => for row in rows.iter() {
                row.len().hash(&mut hash);
                for &leaf in row { self.census_leaf_at(leaf, loose, depth).hash(&mut hash) }
            },
            _ => 9u8.hash(&mut hash),
        }
        hash.finish()
    }

    fn census_key(&self, key: Option<&Key>) -> u64 {
        let mut hash = FxHasher::default();
        match key {
            Some(Key::Symbol(node)) => self.census_node(*node, &mut hash),
            Some(key) => format!("{key:?}").hash(&mut hash),
            None => {}
        }
        hash.finish()
    }

    /// A whole file's keys.
    pub(crate) fn census_file(&self, file: u32) -> (u64, u64, String) {
        let mut hash = FxHasher::default();
        let path = self.world.path(file).map(|path| path.display().to_string()).unwrap_or_default();
        path.hash(&mut hash);
        let key = hash.finish();
        (key, key, path)
    }

    /// `leaf`'s strict key, loose key and label.
    pub fn census_leaf(&self, leaf: Leaf) -> (u64, u64, String) {
        let text: String = self.census_text(leaf).chars().take(70).map(|c| if c == '\n' { ' ' } else { c }).collect();
        (self.census_leaf_at(leaf, false, 0), self.census_leaf_at(leaf, true, 0), text)
    }

    /// A pair of values' keys and label.
    pub(crate) fn census_values(&self, expected: &NodeValue, actual: &NodeValue) -> (u64, u64, String) {
        let both = |loose| {
            let mut hash = FxHasher::default();
            self.census_value(expected, loose, 0).hash(&mut hash);
            self.census_value(actual, loose, 0).hash(&mut hash);
            hash.finish()
        };
        (both(false), both(true), String::new())
    }

    /// A memo key's census key, where its type is known; a key of another
    /// type never counts as a repeat.
    pub(crate) fn census_any(&self, key: &dyn Any) -> (u64, u64, String) {
        static UNIQUE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 40);
        let with = |(strict, loose, label): (u64, u64, String), extra: u64| (strict ^ extra.wrapping_mul(0x9E37_79B9), loose ^ extra.wrapping_mul(0x9E37_79B9), label);
        if let Some(leaf) = key.downcast_ref::<Leaf>() { return self.census_leaf(*leaf) }
        if let Some((leaf, id)) = key.downcast_ref::<(Leaf, u16)>() { return with(self.census_leaf(*leaf), u64::from(*id) + 1) }
        if let Some((leaf, member)) = key.downcast_ref::<(Leaf, Key)>() {
            let (strict, loose, label) = with(self.census_leaf(*leaf), self.census_key(Some(member)));
            return (strict, loose, format!(".{member:?} of {label}"));
        }
        let unique = UNIQUE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        (unique, unique, String::new())
    }
}
