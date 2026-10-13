//! Packed composite values: config constants, evaluation scratch, and
//! per-file attribute escapes. A handle never owns or borrows a payload.

use super::V;
use crate::expr::{value::Key, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle(u32);

const CELL_OFFSET_MASK: u32 = (1 << 30) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Region {
    Config = 0,
    Scratch = 1,
    File = 2,
}

impl Handle {
    pub(super) fn bits(self) -> u32 { self.0 }
    pub(super) fn from_bits(bits: u32) -> Self { Self(bits) }
    fn region(self) -> Region {
        match self.0 >> 30 {
            0 => Region::Config,
            1 => Region::Scratch,
            2 => Region::File,
            _ => unreachable!("invalid VM value region"),
        }
    }
    fn offset(self) -> usize { (self.0 & CELL_OFFSET_MASK) as usize }
    pub(super) fn is_scratch(self) -> bool { self.region() == Region::Scratch }
}

/// The bit marking a [`V::Text`] handle as evaluation scratch, not file.
pub(super) const SCRATCH_TEXT: u32 = 1 << 31;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PackError {
    Cells,
    Text,
    RuntimeShapes,
    ConfigShapes,
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Cells => "VM value cell arena limit exceeded",
            Self::Text => "VM text arena limit exceeded",
            Self::RuntimeShapes => "VM runtime record shape limit exceeded",
            Self::ConfigShapes => "VM config record shape limit exceeded",
        })
    }
}

fn cells_end(offset: usize, items: usize) -> Result<usize, PackError> {
    offset.checked_add(items.checked_add(1).ok_or(PackError::Cells)?)
        .filter(|&end| end <= CELL_OFFSET_MASK as usize).ok_or(PackError::Cells)
}

fn text_end(offset: usize, bytes: usize) -> Result<usize, PackError> {
    offset.checked_add(bytes).filter(|&end| end < SCRATCH_TEXT as usize).ok_or(PackError::Text)
}

fn runtime_shape_id(count: usize) -> Result<u16, PackError> {
    RUNTIME_SHAPE.checked_add(u16::try_from(count).map_err(|_| PackError::RuntimeShapes)?)
        .filter(|&id| id < u16::MAX).ok_or(PackError::RuntimeShapes)
}

#[derive(Default, Debug)]
pub(super) struct Arena {
    cells: Vec<u64>,
    texts: Vec<u8>,
}

impl Arena {
    /// Empties the arena, keeping its capacity for reuse.
    pub(super) fn clear(&mut self) {
        self.cells.clear();
        self.texts.clear();
    }

    pub(super) fn try_allocate(&mut self, region: Region, items: &[V]) -> Result<Handle, PackError> {
        let offset = self.cells.len();
        cells_end(offset, items.len())?;
        self.cells.push(items.len() as u64);
        self.cells.extend(items.iter().copied().map(super::records::encode));
        Ok(Handle(offset as u32 | (region as u32) << 30))
    }

    fn len(&self, handle: Handle) -> usize { self.cells[handle.offset()] as usize }
    fn item(&self, handle: Handle, at: usize) -> V {
        super::records::stored(self.cells[handle.offset() + 1 + at])
    }
    pub(super) fn bytes(&self) -> usize { self.cells.capacity() * 8 }
    pub(super) fn text_bytes(&self) -> usize { self.texts.capacity() }
    pub(super) fn used_bytes(&self) -> usize { self.cells.len() * 8 + self.texts.len() }

    /// Stores `text` (a borrow when it lies in `source`); its offset, below
    /// [`SCRATCH_TEXT`].
    pub(super) fn try_text(&mut self, text: &str, source: &str) -> Result<u32, PackError> {
        let len = u32::try_from(text.len()).map_err(|_| PackError::Text)?;
        let source_offset = (text.as_ptr() as usize).checked_sub(source.as_ptr() as usize)
            .filter(|&start| start.checked_add(text.len()).is_some_and(|end| end <= source.len()))
            .and_then(|start| u32::try_from(start).ok());
        let entry_len = 5usize.checked_add(source_offset.map_or(text.len(), |_| 4)).ok_or(PackError::Text)?;
        let offset = u32::try_from(self.texts.len()).map_err(|_| PackError::Text)?;
        text_end(self.texts.len(), entry_len)?;
        self.texts.push(u8::from(source_offset.is_none()));
        self.texts.extend_from_slice(&len.to_le_bytes());
        match source_offset {
            Some(start) => self.texts.extend_from_slice(&start.to_le_bytes()),
            None => self.texts.extend_from_slice(text.as_bytes()),
        }
        Ok(offset)
    }

    fn text_range(&self, handle: u32) -> (bool, std::ops::Range<usize>) {
        let at = handle as usize;
        let len = u32::from_le_bytes(self.texts[at + 1..at + 5].try_into().expect("text header holds a length")) as usize;
        if self.texts[at] == 0 {
            let start = u32::from_le_bytes(self.texts[at + 5..at + 9].try_into().expect("source text holds an offset")) as usize;
            (true, start..start + len)
        } else {
            (false, at + 5..at + 5 + len)
        }
    }

    fn text_value<'a>(&'a self, handle: u32, source: &'a str) -> &'a str {
        let (borrowed, range) = self.text_range(handle);
        if borrowed { &source[range] } else { std::str::from_utf8(&self.texts[range]).expect("stored text is UTF-8") }
    }

    /// Copies a text entry as stored (a borrow stays a borrow).
    fn copy_text(&self, handle: u32, target: &mut Arena) -> Result<u32, PackError> {
        let (borrowed, range) = self.text_range(handle);
        let at = handle as usize;
        let end = if borrowed { at + 9 } else { range.end };
        let offset = u32::try_from(target.texts.len()).map_err(|_| PackError::Text)?;
        text_end(target.texts.len(), end - at)?;
        target.texts.extend_from_slice(&self.texts[at..end]);
        Ok(offset)
    }

    /// Only scratch payloads need copying. Existing config/file handles
    /// remain shared; nested scratch descendants are promoted recursively.
    pub(super) fn try_promote(&self, value: V, target: &mut Arena) -> Result<V, PackError> {
        let (shape, handle) = match value {
            V::Text(handle) if handle & SCRATCH_TEXT != 0 => return Ok(V::Text(self.copy_text(handle & !SCRATCH_TEXT, target)?)),
            V::List(handle) => (None, handle),
            V::Record(shape, handle) => (Some(shape), handle),
            scalar => return Ok(scalar),
        };
        if handle.region() != Region::Scratch { return Ok(value); }
        let items = (0..self.len(handle)).map(|at| self.try_promote(self.item(handle, at), target)).collect::<Result<Vec<_>, _>>()?;
        let handle = target.try_allocate(Region::File, &items)?;
        Ok(shape.map_or(V::List(handle), |shape| V::Record(shape, handle)))
    }
}

/// Keys retain written order, including duplicates: reads use the first.
/// One registry is shared by all lowered attributes and rule conditions.
#[derive(Default, Debug)]
pub struct Config {
    pub(crate) lowering_errors: Vec<String>,
    shapes: Vec<Box<[Key]>>,
    shape_ids: crate::fx_hash::FxHashMap<Vec<String>, u16>,
    pub(super) constants: Arena,
    fact_shapes: crate::fx_hash::FxHashMap<usize, FactShape>,
    /// Lowered `fn` bodies by index (`None` while being lowered).
    functions: crate::fx_hash::FxHashMap<u32, Option<super::VExpr>>,
    function_errors: crate::fx_hash::FxHashMap<u32, String>,
    /// Shapes first met while linting (a native fact's records, say), ids
    /// from [`RUNTIME_SHAPE`]: records always pack. Append-only, shared by
    /// the lint threads.
    runtime_shapes: std::sync::RwLock<RuntimeShapes>,
}

/// The first id of a shape registered while linting.
const RUNTIME_SHAPE: u16 = 0x8000;

#[derive(Default, Debug)]
struct RuntimeShapes {
    keys: Vec<std::sync::Arc<[Key]>>,
    ids: crate::fx_hash::FxHashMap<Vec<String>, u16>,
}

/// A shape's keys, from the config or the runtime registry.
pub(super) enum Keys<'a> {
    Config(&'a [Key]),
    Runtime(std::sync::Arc<[Key]>),
}

impl std::ops::Deref for Keys<'_> {
    type Target = [Key];
    fn deref(&self) -> &[Key] {
        match self {
            Keys::Config(keys) => keys,
            Keys::Runtime(keys) => keys,
        }
    }
}

#[derive(Debug)]
enum FactShape {
    Scalar,
    Record(u16, Box<[FactShape]>),
    List(Box<FactShape>),
}

impl Config {
    pub(crate) fn clear_runtime(&mut self) {
        *self.runtime_shapes.get_mut().expect("shape registry") = RuntimeShapes::default();
    }

    pub(crate) fn take_lowering_errors(&mut self, owner: &str) -> Vec<String> {
        self.lowering_errors.drain(..).map(|error| format!("{owner}: {error}")).collect()
    }
    pub(super) fn register_fact(&mut self, fact: usize) -> Option<()> {
        fn register(config: &mut Config, shape: crate::expr::facts::Shape) -> Option<FactShape> {
            Some(match shape {
                crate::expr::facts::Shape::Scalar => FactShape::Scalar,
                crate::expr::facts::Shape::Record(fields) => {
                    let shape = config.shape(fields.iter().map(|(key, _)| (*key).into()))?;
                    let fields = fields.iter().map(|(_, shape)| register(config, *shape)).collect::<Option<Box<[_]>>>()?;
                    FactShape::Record(shape, fields)
                }
                crate::expr::facts::Shape::List(item) => FactShape::List(Box::new(register(config, *item)?)),
            })
        }
        if !self.fact_shapes.contains_key(&fact) {
            let shape = register(self, crate::expr::facts::FACTS[fact].shape)?;
            self.fact_shapes.insert(fact, shape);
        }
        Some(())
    }

    pub(super) fn try_fact_value(&self, value: &Value, fact: usize, projected: bool, listed: bool, arena: &mut Arena, source: &str) -> Result<V, PackError> {
        fn convert(config: &Config, value: &Value, shape: &FactShape, arena: &mut Arena, source: &str) -> Result<V, PackError> {
            Ok(match value {
                Value::Str(text) => V::Text(arena.try_text(text, source)?),
                Value::Node(node, _) => V::Node(*node),
                Value::Record(entries) => match shape {
                    FactShape::Record(shape, fields) if {
                        let keys = config.shape_keys(*shape);
                        entries.len() == keys.len() && entries.iter().zip(keys.iter()).all(|((key, _), expected)| key == expected)
                    } => {
                        let items = entries.iter().zip(fields.iter()).map(|((_, value), field)| convert(config, value, field, arena, source)).collect::<Result<Vec<_>, _>>()?;
                        V::Record(*shape, arena.try_allocate(Region::File, &items)?)
                    }
                    // Undeclared or differing: packed as it comes.
                    _ => config.try_value_into(value, arena, Region::File, source)?,
                },
                Value::List(items) => match shape {
                    FactShape::List(shape) => {
                        let items = items.iter().map(|value| convert(config, value, shape, arena, source)).collect::<Result<Vec<_>, _>>()?;
                        V::List(arena.try_allocate(Region::File, &items)?)
                    }
                    _ => config.try_value_into(value, arena, Region::File, source)?,
                },
                Value::Null => V::Null,
                Value::Bool(value) => V::Bool(*value),
                Value::Num(value) => V::Num(*value),
            })
        }
        let scalar = FactShape::Scalar;
        let shape = if projected { &scalar } else { self.fact_shapes.get(&fact).unwrap_or(&scalar) };
        // Multi-field reads wrap results; that list is not the fact's own shape.
        if listed {
            if let Value::List(items) = value {
                let items = items.iter().map(|value| convert(self, value, shape, arena, source)).collect::<Result<Vec<_>, _>>()?;
                return Ok(V::List(arena.try_allocate(Region::File, &items)?));
            }
        }
        convert(self, value, shape, arena, source)
    }
    /// A value packed into `arena` (scratch or the file's).
    pub(super) fn try_value_into(&self, value: &Value, arena: &mut Arena, region: Region, source: &str) -> Result<V, PackError> {
        Ok(match value {
            Value::Str(text) => V::Text(arena.try_text(text, source)? | if region == Region::Scratch { SCRATCH_TEXT } else { 0 }),
            Value::Node(node, _) => V::Node(*node),
            Value::List(items) => {
                let items = items.iter().map(|item| self.try_value_into(item, arena, region, source)).collect::<Result<Vec<_>, _>>()?;
                V::List(arena.try_allocate(region, &items)?)
            }
            Value::Record(entries) => {
                let shape = self.record_shape(entries)?;
                let items = entries.iter().map(|(_, item)| self.try_value_into(item, arena, region, source)).collect::<Result<Vec<_>, _>>()?;
                V::Record(shape, arena.try_allocate(region, &items)?)
            }
            Value::Null => V::Null,
            Value::Bool(value) => V::Bool(*value),
            Value::Num(value) => V::Num(*value),
        })
    }

    /// Claims lowering `fn` `i`: false when it is lowered or being lowered.
    pub(super) fn begin_function(&mut self, i: u32) -> bool {
        match self.functions.entry(i) {
            std::collections::hash_map::Entry::Occupied(_) => false,
            std::collections::hash_map::Entry::Vacant(slot) => { slot.insert(None); true }
        }
    }

    pub(super) fn set_function(&mut self, i: u32, body: super::VExpr) {
        self.functions.insert(i, Some(body));
    }

    pub(super) fn reject_function(&mut self, i: u32, name: &str, start: usize) {
        let message = format!("fn `{name}`: {}", self.lowering_errors[start..].join("; "));
        self.lowering_errors.truncate(start);
        self.lowering_errors.push(message.clone());
        self.function_errors.insert(i, message);
    }

    pub(super) fn function_error(&self, i: u32) -> Option<&str> {
        self.function_errors.get(&i).map(String::as_str)
    }

    /// `fn` `i`'s lowered body.
    pub(super) fn function(&self, i: u32) -> Option<&super::VExpr> {
        self.functions.get(&i)?.as_ref()
    }

    /// A shape's keys.
    pub(super) fn shape_keys(&self, shape: u16) -> Keys<'_> {
        if shape < RUNTIME_SHAPE {
            return Keys::Config(&self.shapes[shape as usize]);
        }
        let shapes = self.runtime_shapes.read().expect("shape registry");
        Keys::Runtime(shapes.keys[(shape - RUNTIME_SHAPE) as usize].clone())
    }

    /// The id of a record's shape, registering it at run time when the
    /// config never named it.
    fn record_shape(&self, entries: &[(Key, Value)]) -> Result<u16, PackError> {
        let names: Vec<String> = entries.iter().map(|(key, _)| key.to_string()).collect();
        if let Some(&id) = self.shape_ids.get(&names) { return Ok(id); }
        if let Some(&id) = self.runtime_shapes.read().expect("shape registry").ids.get(&names) { return Ok(id); }
        let mut shapes = self.runtime_shapes.write().expect("shape registry");
        if let Some(&id) = shapes.ids.get(&names) { return Ok(id); }
        let id = runtime_shape_id(shapes.keys.len())?;
        shapes.keys.push(entries.iter().map(|(key, _)| key.clone()).collect());
        shapes.ids.insert(names, id);
        Ok(id)
    }

    pub(super) fn shape(&mut self, keys: impl Iterator<Item = Key>) -> Option<u16> {
        let keys: Box<[Key]> = keys.collect();
        let names: Vec<String> = keys.iter().map(ToString::to_string).collect();
        if let Some(&id) = self.shape_ids.get(&names) { return Some(id); }
        let Some(id) = u16::try_from(self.shapes.len()).ok().filter(|&id| id < RUNTIME_SHAPE) else {
            self.lowering_errors.push(PackError::ConfigShapes.to_string());
            return None;
        };
        self.shapes.push(keys);
        self.shape_ids.insert(names, id);
        Some(id)
    }

    pub(super) fn constant(&mut self, value: &Value) -> Option<V> {
        Some(match value {
            Value::List(items) => {
                let items = items.iter().map(|v| self.constant(v)).collect::<Option<Vec<_>>>()?;
                V::List(self.constants.try_allocate(Region::Config, &items).map_err(|error| self.lowering_errors.push(error.to_string())).ok()?)
            }
            Value::Record(entries) => {
                let shape = self.shape(entries.iter().map(|(key, _)| key.clone()))?;
                let items = entries.iter().map(|(_, v)| self.constant(v)).collect::<Option<Vec<_>>>()?;
                V::Record(shape, self.constants.try_allocate(Region::Config, &items).map_err(|error| self.lowering_errors.push(error.to_string())).ok()?)
            }
            scalar => super::constant(scalar)?,
        })
    }
}

pub(super) struct View<'a> {
    pub config: &'a Config,
    pub scratch: &'a Arena,
    pub file: &'a Arena,
}

impl View<'_> {
    fn arena(&self, handle: Handle) -> &Arena {
        match handle.region() {
            Region::Config => &self.config.constants,
            Region::Scratch => self.scratch,
            Region::File => self.file,
        }
    }

    /// The pool holding a text handle, and the entry's offset there.
    fn text_pool(&self, handle: u32) -> (&Arena, u32) {
        if handle & SCRATCH_TEXT != 0 { (self.scratch, handle & !SCRATCH_TEXT) } else { (self.file, handle) }
    }

    fn pooled<'a>(&'a self, handle: u32, source: &'a str) -> &'a str {
        let (pool, at) = self.text_pool(handle);
        pool.text_value(at, source)
    }

    pub fn len(&self, handle: Handle) -> usize { self.arena(handle).len(handle) }
    pub fn item(&self, handle: Handle, at: usize) -> V { self.arena(handle).item(handle, at) }

    pub fn key(&self, shape: u16, handle: Handle, key: &str) -> V {
        self.config.shape_keys(shape).iter().position(|k| &**k == key)
            .map_or(V::Null, |at| self.item(handle, at))
    }

    pub fn truthy(&self, value: V, tree: &crate::grammar::Tree) -> bool {
        match value {
            V::List(handle) | V::Record(_, handle) => self.len(handle) != 0,
            V::Text(handle) => {
                let (pool, at) = self.text_pool(handle);
                !pool.text_range(at).1.is_empty()
            }
            scalar => scalar.truthy(tree),
        }
    }

    pub fn equal(&self, a: V, b: V, tree: &crate::grammar::Tree, source: &str) -> bool {
        match (a, b) {
            (V::List(a), V::List(b)) => self.len(a) == self.len(b)
                && (0..self.len(a)).all(|at| self.equal(self.item(a, at), self.item(b, at), tree, source)),
            (V::Record(sa, a), V::Record(sb, b)) => self.len(a) == self.len(b)
                && self.config.shape_keys(sa).iter().enumerate().all(|(at, key)| {
                    self.config.shape_keys(sb).iter().enumerate().any(|(bt, other)| {
                        key == other && self.equal(self.item(a, at), self.item(b, bt), tree, source)
                    })
                }),
            (a, b) if matches!(a, V::Text(_)) || matches!(b, V::Text(_)) => match (a, b) {
                (V::Num(number), text) | (text, V::Num(number)) => self.text(text, tree, source)
                    .is_some_and(|text| text.trim().parse::<f64>().is_ok_and(|other| number == other)),
                (V::Null, text) | (text, V::Null) => self.text(text, tree, source).is_some_and(str::is_empty),
                (a, b) => self.text(a, tree, source).zip(self.text(b, tree, source)).is_some_and(|(a, b)| a == b),
            },
            (a, b) => a.loose_eq(b, tree, source),
        }
    }

    pub fn value<'s>(&self, value: V, tree: &crate::grammar::Tree, source: &'s str) -> Value<'s> {
        match value {
            V::List(handle) => Value::list((0..self.len(handle)).map(|at| self.value(self.item(handle, at), tree, source)).collect()),
            V::Record(shape, handle) => Value::record(self.config.shape_keys(shape).iter().enumerate()
                .map(|(at, key)| (key.clone(), self.value(self.item(handle, at), tree, source))).collect::<Vec<_>>()),
            V::Text(handle) => {
                let (pool, at) = self.text_pool(handle);
                let (borrowed, range) = pool.text_range(at);
                if borrowed { Value::from(&source[range]) } else { Value::text_copy(pool.text_value(at, source)) }
            }
            scalar => scalar.value(tree, source),
        }
    }

    pub fn text<'a>(&'a self, value: V, tree: &crate::grammar::Tree, source: &'a str) -> Option<&'a str> {
        match value {
            V::Text(handle) => Some(self.pooled(handle, source)),
            scalar => scalar.text(tree, source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_bounds_report_the_limit_without_wrapping_offsets_or_ids() {
        let cell_limit = CELL_OFFSET_MASK as usize;
        assert_eq!(cells_end(cell_limit - 1, 0), Ok(cell_limit));
        assert_eq!(cells_end(cell_limit, 0), Err(PackError::Cells));
        assert_eq!(cells_end(0, usize::MAX), Err(PackError::Cells));
        assert_eq!(text_end(SCRATCH_TEXT as usize - 2, 1), Ok(SCRATCH_TEXT as usize - 1));
        assert_eq!(text_end(SCRATCH_TEXT as usize - 1, 1), Err(PackError::Text));
        assert_eq!(text_end(usize::MAX, 1), Err(PackError::Text));
        assert_eq!(runtime_shape_id(usize::from(u16::MAX - RUNTIME_SHAPE - 1)), Ok(u16::MAX - 1));
        assert_eq!(runtime_shape_id(usize::from(u16::MAX - RUNTIME_SHAPE)), Err(PackError::RuntimeShapes));
        assert_eq!(runtime_shape_id(usize::MAX), Err(PackError::RuntimeShapes));
    }

    #[test]
    fn native_text_keeps_source_borrows_and_owned_values_after_pool_growth() {
        let source = "a\u{1f600}42";
        let mut file = Arena::default();
        let borrowed = file.try_text(&source[1..5], source).unwrap();
        let owned = {
            let text = String::from("5.5");
            file.try_text(&text, source).unwrap()
        };
        let empty = file.try_text("", source).unwrap();
        for _ in 0..1000 { file.try_text("padding", source).unwrap(); }
        let config = Config::default();
        let scratch = Arena::default();
        let tree = crate::grammar::Tree::from_native(vec![crate::grammar::Node {
            matcher: crate::grammar::ROOT, start: 0, end: source.len() as u32, children: Vec::new(),
        }], 0);
        let view = View { config: &config, scratch: &scratch, file: &file };
        assert_eq!(view.text(V::Text(borrowed), &tree, source), Some(&source[1..5]));
        assert!(matches!(view.value(V::Text(borrowed), &tree, source), Value::Str(crate::expr::value::Text::Borrowed(_))));
        assert_eq!(view.value(V::Text(owned), &tree, source), Value::from("5.5"));
        assert!(view.equal(V::Text(owned), V::Num(5.5), &tree, source));
        assert!(!view.truthy(V::Text(empty), &tree));
        assert!(view.equal(V::Text(empty), V::Null, &tree, source));
    }

    #[test]
    fn attribute_escapes_keep_nested_values_after_scratch_is_dropped() {
        let mut config = Config::default();
        let shape = config.shape(["value", "constant"].into_iter().map(Into::into)).unwrap();
        let constant = config.constant(&Value::list(vec![Value::Num(5.0)])).unwrap();
        let mut file = Arena::default();
        let escaped = {
            let mut scratch = Arena::default();
            let list = scratch.try_allocate(Region::Scratch, &[V::Num(2.0), constant]).unwrap();
            let record = scratch.try_allocate(Region::Scratch, &[V::List(list), constant]).unwrap();
            scratch.try_promote(V::Record(shape, record), &mut file).unwrap()
        };
        let scratch = Arena::default();
        for _ in 0..1000 { file.try_allocate(Region::File, &[V::Null; 7]).unwrap(); }
        let view = View { config: &config, scratch: &scratch, file: &file };
        let V::Record(shape, handle) = escaped else { panic!("record"); };
        let V::List(list) = view.key(shape, handle, "value") else { panic!("nested list"); };
        assert_eq!(view.item(list, 0), V::Num(2.0));
        assert_eq!(view.item(list, 1), constant);
        assert_eq!(view.key(shape, handle, "constant"), constant);
        assert_eq!(view.key(shape, handle, "missing"), V::Null);
    }
}
