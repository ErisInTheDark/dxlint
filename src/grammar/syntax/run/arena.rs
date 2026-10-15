use std::marker::PhantomData;

/// Entry id meaning "none".
const NONE: u32 = u32::MAX;

/// A list handle: ids into its [`Chain`], copied freely (12 bytes).
#[derive(Debug, PartialEq, Eq)]
pub(super) struct List<T: Copy> {
    last: u32,
    first: u32,
    len: u32,
    marker: PhantomData<T>,
}

impl<T: Copy> Clone for List<T> {
    fn clone(&self) -> Self { *self }
}

impl<T: Copy> Copy for List<T> {}

impl<T: Copy> Default for List<T> {
    fn default() -> Self { Self { last: NONE, first: NONE, len: 0, marker: PhantomData } }
}

impl<T: Copy> List<T> {
    pub fn is_empty(self) -> bool { self.last == NONE }
    pub fn len(self) -> usize { self.len as usize }
}

/// Entries are never changed once written, so lists (memo answers, sibling
/// branches) share them freely. A join stands for one list followed by
/// another without copying either.
enum Entry<T: Copy> {
    Item { previous: u32, value: T },
    Join { left: u32, right: u32 },
}

pub(super) struct Chain<T: Copy> {
    entries: Vec<Entry<T>>,
    pending: Vec<T>,
}

impl<T: Copy> Default for Chain<T> {
    fn default() -> Self { Self { entries: Vec::new(), pending: Vec::new() } }
}

impl<T: Copy> Chain<T> {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.pending.clear();
    }

    pub fn len(&self) -> usize { self.entries.len() }

    fn next_id(&self) -> u32 {
        u32::try_from(self.entries.len()).ok().filter(|&id| id != NONE).expect("list entries fit u32 ids")
    }

    pub fn push(&mut self, list: &mut List<T>, value: T) {
        let id = self.next_id();
        self.entries.push(Entry::Item { previous: list.last, value });
        let first = if list.is_empty() { id } else { list.first };
        *list = List { last: id, first, len: list.len + 1, marker: PhantomData };
    }

    pub fn single(&mut self, value: T) -> List<T> {
        let mut list = List::default();
        self.push(&mut list, value);
        list
    }

    /// The first value of `list`.
    pub fn first(&self, list: List<T>) -> Option<T> {
        match self.entries.get(list.first as usize)? {
            Entry::Item { value, .. } => Some(*value),
            Entry::Join { .. } => unreachable!("a list's first entry is an item"),
        }
    }

    /// `list` followed by `next`, in constant time.
    pub fn append(&mut self, list: &mut List<T>, next: List<T>) {
        if next.is_empty() { return; }
        if list.is_empty() { *list = next; return; }
        let join = self.next_id();
        self.entries.push(Entry::Join { left: list.last, right: next.last });
        *list = List { last: join, first: list.first, len: list.len + next.len, marker: PhantomData };
    }

    pub fn filter(&mut self, list: List<T>, keep: impl Fn(T) -> bool) -> List<T> {
        let mut pending = std::mem::take(&mut self.pending);
        pending.clear();
        pending.extend(self.reversed(list).filter(|&value| keep(value)));
        let mut filtered = List::default();
        for index in (0..pending.len()).rev() {
            self.push(&mut filtered, pending[index]);
        }
        self.pending = pending;
        filtered
    }

    /// The values last to first.
    pub fn reversed(&self, list: List<T>) -> Reversed<'_, T> {
        Reversed { chain: self, at: list.last, stack: Stack::default() }
    }

    pub fn write_ordered(&self, list: List<T>, values: &mut Vec<T>) {
        values.clear();
        values.extend(self.reversed(list));
        values.reverse();
    }

    #[cfg(test)]
    pub fn ordered(&self, list: List<T>) -> Vec<T> {
        let mut values = Vec::new();
        self.write_ordered(list, &mut values);
        values
    }
}

/// Pending left halves of joins: a few inline, deeper ones spilled.
#[derive(Default)]
struct Stack {
    inline: [u32; 8],
    len: usize,
    spilled: Vec<u32>,
}

impl Stack {
    fn push(&mut self, id: u32) {
        if self.len < self.inline.len() && self.spilled.is_empty() {
            self.inline[self.len] = id;
            self.len += 1;
        } else {
            self.spilled.push(id);
        }
    }

    fn pop(&mut self) -> Option<u32> {
        if let Some(id) = self.spilled.pop() { return Some(id) }
        self.len = self.len.checked_sub(1)?;
        Some(self.inline[self.len])
    }
}

pub(super) struct Reversed<'a, T: Copy> {
    chain: &'a Chain<T>,
    at: u32,
    stack: Stack,
}

impl<T: Copy> Iterator for Reversed<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        loop {
            let id = match std::mem::replace(&mut self.at, NONE) {
                NONE => self.stack.pop()?,
                id => id,
            };
            match self.chain.entries[id as usize] {
                Entry::Item { previous, value } => {
                    self.at = previous;
                    return Some(value);
                }
                Entry::Join { left, right } => {
                    self.stack.push(left);
                    self.at = right;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branches_keep_shared_captures_after_append_and_filter() {
        let mut chain = Chain::default();
        let shared = chain.single(7);
        let mut left = shared;
        let mut right = shared;
        let mut next = chain.single(11);
        chain.push(&mut next, 13);
        chain.append(&mut left, next);
        chain.push(&mut right, 17);
        let filtered = chain.filter(left, |value| value != 11);
        assert_eq!(chain.ordered(shared), [7]);
        assert_eq!(chain.ordered(left), [7, 11, 13]);
        assert_eq!(chain.ordered(right), [7, 17]);
        assert_eq!(chain.ordered(next), [11, 13]);
        assert_eq!(chain.ordered(filtered), [7, 13]);
        assert_eq!(chain.first(left), Some(7));
        assert_eq!(chain.first(next), Some(11));
        assert_eq!(chain.first(filtered), Some(7));
        assert_eq!(chain.first(List::default()), None);
    }

    #[test]
    fn joins_nest_deeply_and_keep_order() {
        let mut chain = Chain::default();
        let mut whole = List::default();
        for value in 0..100 {
            let part = chain.single(value);
            let mut grown = whole;
            chain.append(&mut grown, part);
            whole = grown;
        }
        let mut pushed = whole;
        chain.push(&mut pushed, 100);
        assert_eq!(chain.ordered(whole), (0..100).collect::<Vec<_>>());
        assert_eq!(chain.ordered(pushed), (0..=100).collect::<Vec<_>>());
        assert_eq!(whole.len(), 100);
        assert_eq!(chain.first(pushed), Some(0));
    }
}
