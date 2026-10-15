#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct List<T: Copy> {
    last: Option<usize>,
    first: Option<T>,
    len: usize,
}

impl<T: Copy> Default for List<T> {
    fn default() -> Self { Self { last: None, first: None, len: 0 } }
}

impl<T: Copy> List<T> {
    pub fn is_empty(self) -> bool { self.last.is_none() }
    pub fn len(self) -> usize { self.len }
    pub fn first(&self) -> Option<&T> { self.first.as_ref() }
}

/// Entries are never changed once written, so lists (memo answers, sibling
/// branches) share them freely. A join stands for one list followed by
/// another without copying either.
enum Entry<T: Copy> {
    Item { previous: Option<usize>, value: T },
    Join { left: usize, right: usize },
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

    pub fn push(&mut self, list: &mut List<T>, value: T) {
        let last = self.entries.len();
        self.entries.push(Entry::Item { previous: list.last, value });
        *list = List { last: Some(last), first: list.first.or(Some(value)), len: list.len + 1 };
    }

    pub fn single(&mut self, value: T) -> List<T> {
        let mut list = List::default();
        self.push(&mut list, value);
        list
    }

    /// `list` followed by `next`, in constant time.
    pub fn append(&mut self, list: &mut List<T>, next: List<T>) {
        let (Some(left), Some(right)) = (list.last, next.last) else {
            if list.is_empty() { *list = next; }
            return;
        };
        let join = self.entries.len();
        self.entries.push(Entry::Join { left, right });
        *list = List { last: Some(join), first: list.first, len: list.len + next.len };
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
    inline: [usize; 8],
    len: usize,
    spilled: Vec<usize>,
}

impl Stack {
    fn push(&mut self, id: usize) {
        if self.len < self.inline.len() && self.spilled.is_empty() {
            self.inline[self.len] = id;
            self.len += 1;
        } else {
            self.spilled.push(id);
        }
    }

    fn pop(&mut self) -> Option<usize> {
        if let Some(id) = self.spilled.pop() { return Some(id) }
        self.len = self.len.checked_sub(1)?;
        Some(self.inline[self.len])
    }
}

pub(super) struct Reversed<'a, T: Copy> {
    chain: &'a Chain<T>,
    at: Option<usize>,
    stack: Stack,
}

impl<T: Copy> Iterator for Reversed<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        loop {
            let id = match self.at.take() {
                Some(id) => id,
                None => self.stack.pop()?,
            };
            match self.chain.entries[id] {
                Entry::Item { previous, value } => {
                    self.at = previous;
                    return Some(value);
                }
                Entry::Join { left, right } => {
                    self.stack.push(left);
                    self.at = Some(right);
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
    }
}
