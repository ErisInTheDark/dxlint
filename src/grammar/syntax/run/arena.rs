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

struct Entry<T: Copy> {
    previous: Option<usize>,
    value: T,
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

    pub fn push(&mut self, list: &mut List<T>, value: T) {
        let last = self.entries.len();
        self.entries.push(Entry { previous: list.last, value });
        *list = List { last: Some(last), first: list.first.or(Some(value)), len: list.len + 1 };
    }

    pub fn single(&mut self, value: T) -> List<T> {
        let mut list = List::default();
        self.push(&mut list, value);
        list
    }

    pub fn append(&mut self, list: &mut List<T>, next: List<T>) {
        if next.is_empty() { return; }
        if list.is_empty() { *list = next; return; }
        self.pending.clear();
        let mut at = next.last;
        while let Some(id) = at {
            let entry = &self.entries[id];
            self.pending.push(entry.value);
            at = entry.previous;
        }
        for index in (0..self.pending.len()).rev() {
            self.push(list, self.pending[index]);
        }
    }

    pub fn filter(&mut self, list: List<T>, keep: impl Fn(T) -> bool) -> List<T> {
        self.pending.clear();
        let mut at = list.last;
        while let Some(id) = at {
            let entry = &self.entries[id];
            if keep(entry.value) { self.pending.push(entry.value); }
            at = entry.previous;
        }
        let mut filtered = List::default();
        for index in (0..self.pending.len()).rev() {
            self.push(&mut filtered, self.pending[index]);
        }
        filtered
    }

    pub fn reversed(&self, list: List<T>) -> impl Iterator<Item = T> + '_ {
        let mut at = list.last;
        std::iter::from_fn(move || {
            let entry = &self.entries[at?];
            at = entry.previous;
            Some(entry.value)
        })
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
}
