//! (instruction x position) bitsets for the interpreter: per-run visited
//! sets, and per-matcher sets of states known to be dead.

const PAGE_WORDS: usize = 64;

/// Positions sharing one page of bits for a program `width` long.
fn per_page(width: usize) -> usize {
    (PAGE_WORDS * 64 / width.max(1)).max(1)
}

/// Lazily allocated pages, aligned to absolute positions (so they can be
/// merged into [`Dead`]) and recycled across runs.
#[derive(Default)]
pub struct Visited {
    width: usize,
    /// Absolute position of the first page.
    offset: usize,
    per_page: usize,
    words: usize,
    pages: Vec<Option<Box<[u64]>>>,
}

impl Visited {
    pub fn reset(&mut self, width: usize, start: usize) {
        let width = width.max(1);
        self.width = width;
        self.per_page = per_page(width);
        self.offset = start - start % self.per_page;
        self.words = (self.per_page * width).div_ceil(64).max(PAGE_WORDS);
    }

    pub fn recycle(&mut self, free: &mut Vec<Box<[u64]>>) {
        free.extend(self.pages.drain(..).flatten().filter(|p| p.len() == PAGE_WORDS));
    }

    /// Marks (pc, pos); false if it was already marked.
    #[inline]
    pub fn insert(&mut self, pc: usize, pos: usize, free: &mut Vec<Box<[u64]>>) -> bool {
        let rel = pos - self.offset;
        let idx = rel / self.per_page;
        if idx >= self.pages.len() {
            self.pages.resize(idx + 1, None);
        }
        let words = self.words;
        let page = self.pages[idx].get_or_insert_with(|| match free.pop().filter(|p| p.len() == words) {
            Some(mut p) => {
                p.fill(0);
                p
            }
            None => vec![0; words].into_boxed_slice(),
        });
        let bit = (rel % self.per_page) * self.width + pc;
        let (w, mask) = (bit / 64, 1u64 << (bit % 64));
        let fresh = page[w] & mask == 0;
        page[w] |= mask;
        fresh
    }
}

/// States a matcher can never succeed from, learned from failed runs.
///
/// A memoised matcher run is a pure function of its start position, so
/// once a run fails, every (instruction, position) it explored leads only
/// to failure for any later run of the same matcher. Without this, every
/// unclosed `(` or `"` would rescan to the end of the file: O(n²).
pub struct Dead {
    width: usize,
    per_page: usize,
    pages: Vec<Option<Box<[u64]>>>,
}

impl Dead {
    pub fn new(width: usize, text_len: usize) -> Dead {
        let width = width.max(1);
        let per_page = per_page(width);
        Dead { width, per_page, pages: vec![None; text_len / per_page + 1] }
    }

    #[inline]
    pub fn contains(&self, pc: usize, pos: usize) -> bool {
        match self.pages.get(pos / self.per_page) {
            Some(Some(page)) => {
                let bit = (pos % self.per_page) * self.width + pc;
                page[bit / 64] & (1 << (bit % 64)) != 0
            }
            _ => false,
        }
    }

    pub fn absorb(&mut self, v: &Visited) {
        let first = v.offset / self.per_page;
        for (i, page) in v.pages.iter().enumerate() {
            let Some(page) = page else { continue };
            let Some(slot) = self.pages.get_mut(first + i) else { continue };
            match slot {
                Some(dead) => dead.iter_mut().zip(page.iter()).for_each(|(d, p)| *d |= *p),
                None => *slot = Some(page.clone()),
            }
        }
    }
}
