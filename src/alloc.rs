//! The binary's allocator: small blocks come from size-class free lists in
//! sharded pools, everything else from the system allocator.
//!
//! Linting allocates millions of short-lived small values (records, lists,
//! texts, tree vectors). On Windows the system allocator is one process heap
//! every worker thread contends on, and memory it returns to the OS faults
//! back in for the next file. Here each thread takes its own pool (shards are
//! handed out round-robin, so concurrent workers rarely share one), a freed
//! block goes back to the freeing thread's pool, and pool memory is kept for
//! reuse. Blocks a thread leaves behind stay in its pool for the next thread
//! given that shard.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Blocks up to this size (in 16-byte classes) are pooled.
const SMALL: usize = 2048;
const QUANTUM: usize = 16;
const CLASSES: usize = SMALL / QUANTUM;
/// Bytes a pool takes from the system at a time.
const CHUNK: usize = 256 * 1024;
const SHARDS: usize = 64;

struct Pool {
    /// Per class, a linked list threaded through the free blocks.
    free: [*mut u8; CLASSES],
    bump: *mut u8,
    end: *mut u8,
}

// Pools are only touched under their mutex.
unsafe impl Send for Pool {}

impl Pool {
    const fn new() -> Self {
        Pool { free: [null_mut(); CLASSES], bump: null_mut(), end: null_mut() }
    }

    unsafe fn take(&mut self, class: usize) -> *mut u8 {
        let head = self.free[class];
        if !head.is_null() {
            // SAFETY: a free block's first word holds the next free block.
            self.free[class] = unsafe { *(head as *mut *mut u8) };
            return head;
        }
        let size = (class + 1) * QUANTUM;
        if (self.end as usize) - (self.bump as usize) < size {
            // SAFETY: CHUNK is a valid non-zero size with QUANTUM alignment.
            let chunk = unsafe { System.alloc(Layout::from_size_align_unchecked(CHUNK, QUANTUM)) };
            if chunk.is_null() {
                return null_mut();
            }
            self.bump = chunk;
            // SAFETY: within the chunk just allocated.
            self.end = unsafe { chunk.add(CHUNK) };
        }
        let block = self.bump;
        // SAFETY: `size` bytes remain before `end`.
        self.bump = unsafe { block.add(size) };
        block
    }

    unsafe fn give(&mut self, class: usize, block: *mut u8) {
        // SAFETY: blocks are at least QUANTUM bytes and pointer-aligned.
        unsafe { *(block as *mut *mut u8) = self.free[class] };
        self.free[class] = block;
    }
}

static POOLS: [Mutex<Pool>; SHARDS] = [const { Mutex::new(Pool::new()) }; SHARDS];
static NEXT_SHARD: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// This thread's shard, plus one (zero: not yet chosen).
    static SHARD: Cell<usize> = const { Cell::new(0) };
}

fn shard() -> &'static Mutex<Pool> {
    let index = SHARD
        .try_with(|s| {
            if s.get() == 0 {
                s.set(NEXT_SHARD.fetch_add(1, Ordering::Relaxed) % SHARDS + 1);
            }
            s.get() - 1
        })
        // Thread-local storage is gone while a thread exits: any pool works.
        .unwrap_or(0);
    &POOLS[index]
}

fn class(layout: Layout) -> Option<usize> {
    (layout.size() <= SMALL && layout.align() <= QUANTUM).then(|| layout.size().max(1).div_ceil(QUANTUM) - 1)
}

pub struct Pooled;

unsafe impl GlobalAlloc for Pooled {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match class(layout) {
            // SAFETY: the pool hands out blocks of the class size.
            Some(c) => unsafe { shard().lock().unwrap_or_else(|e| e.into_inner()).take(c) },
            // SAFETY: forwarded unchanged.
            None => unsafe { System.alloc(layout) },
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        match class(layout) {
            Some(_) => {
                // SAFETY: as `alloc`; then zeroed over the requested size.
                let block = unsafe { self.alloc(layout) };
                if !block.is_null() {
                    unsafe { block.write_bytes(0, layout.size()) };
                }
                block
            }
            // SAFETY: forwarded unchanged.
            None => unsafe { System.alloc_zeroed(layout) },
        }
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        match class(layout) {
            // SAFETY: `block` came from `alloc` with this layout, so from a pool.
            Some(c) => unsafe { shard().lock().unwrap_or_else(|e| e.into_inner()).give(c, block) },
            // SAFETY: forwarded unchanged.
            None => unsafe { System.dealloc(block, layout) },
        }
    }

    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `layout.align()` is a valid alignment and the caller keeps
        // `new_size` within isize after rounding.
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        match (class(layout), class(new_layout)) {
            (Some(a), Some(b)) if a == b => block,
            // SAFETY: both sides use the system allocator.
            (None, None) => unsafe { System.realloc(block, layout, new_size) },
            _ => {
                // SAFETY: a fresh block, filled from the old one, which is then freed.
                let moved = unsafe { self.alloc(new_layout) };
                if !moved.is_null() {
                    unsafe {
                        std::ptr::copy_nonoverlapping(block, moved, layout.size().min(new_size));
                        self.dealloc(block, layout);
                    }
                }
                moved
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(size: usize, align: usize) -> Layout {
        Layout::from_size_align(size, align).expect("layout")
    }

    #[test]
    fn blocks_hold_their_bytes_across_reuse_growth_and_threads() {
        let a = Pooled;
        unsafe {
            let mut blocks: Vec<(*mut u8, Layout)> = Vec::new();
            for (i, size) in [1, 15, 16, 17, 100, SMALL, SMALL + 1, 4 * SMALL].into_iter().enumerate() {
                let l = layout(size, if size > 64 { 16 } else { 8 });
                let p = a.alloc(l);
                assert!(!p.is_null() && (p as usize).is_multiple_of(l.align()));
                p.write_bytes(i as u8, size);
                blocks.push((p, l));
            }
            for (i, &(p, l)) in blocks.iter().enumerate() {
                assert!((0..l.size()).all(|k| *p.add(k) == i as u8), "block {i} overwritten");
            }
            // Growth across the small/large boundary keeps the contents.
            let (p, l) = blocks[4];
            let grown = a.realloc(p, l, 3 * SMALL);
            assert!((0..100).all(|k| *grown.add(k) == 4));
            blocks[4] = (grown, layout(3 * SMALL, l.align()));
            // Freed on another thread, reused here.
            let handed: Vec<usize> = blocks.iter().map(|&(p, _)| p as usize).collect();
            let layouts: Vec<Layout> = blocks.iter().map(|&(_, l)| l).collect();
            std::thread::spawn(move || {
                for (p, l) in handed.into_iter().zip(layouts) {
                    Pooled.dealloc(p as *mut u8, l);
                }
            })
            .join()
            .expect("free thread");
            let z = a.alloc_zeroed(layout(48, 8));
            assert!((0..48).all(|k| *z.add(k) == 0));
            a.dealloc(z, layout(48, 8));
        }
    }
}
