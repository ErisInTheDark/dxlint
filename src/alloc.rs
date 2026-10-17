//! The binary's allocator: small blocks come from size-class free lists in
//! pools, everything else from the system allocator.
//!
//! Linting allocates millions of short-lived small values (records, lists,
//! texts, tree vectors). On Windows the system allocator is one process heap
//! every worker thread contends on, and memory it returns to the OS faults
//! back in for the next file. A freed block goes back to the freeing
//! thread's pool, and pool memory is kept for reuse.
//!
//! A worker thread owns a pool outright for as long as it holds [`own_pool`]'s
//! guard: its allocations take no lock at all. The guard hands the pool on to
//! the next worker when it ends. Other threads share mutex-guarded shards
//! (handed out round-robin, so they rarely share one).

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
    /// The pool this thread owns ([`own_pool`]), or null.
    static OWNED: Cell<*mut Pool> = const { Cell::new(null_mut()) };
}

struct Parked(*mut Pool);

// Parked pools belong to no thread until one takes them.
unsafe impl Send for Parked {}

/// Pools whose owners finished, kept for the next worker.
static PARKED: Mutex<Vec<Parked>> = Mutex::new(Vec::new());

/// Gives this thread a pool of its own until the guard drops; small
/// allocations on it then take no lock.
pub fn own_pool() -> OwnedPool {
    let parked = PARKED.lock().unwrap_or_else(|e| e.into_inner()).pop();
    let pool = parked.map_or_else(|| Box::into_raw(Box::new(Pool::new())), |parked| parked.0);
    let previous = OWNED.with(|owned| owned.replace(pool));
    OwnedPool { pool, previous, thread: std::marker::PhantomData }
}

/// A thread's ownership of a pool ([`own_pool`]).
pub struct OwnedPool {
    pool: *mut Pool,
    previous: *mut Pool,
    /// Ownership stays on the thread that took it.
    thread: std::marker::PhantomData<*mut Pool>,
}

impl Drop for OwnedPool {
    fn drop(&mut self) {
        OWNED.with(|owned| owned.set(self.previous));
        PARKED.lock().unwrap_or_else(|e| e.into_inner()).push(Parked(self.pool));
    }
}

/// This thread's owned pool, or null.
fn owned() -> *mut Pool {
    OWNED.try_with(Cell::get).unwrap_or(null_mut())
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

// Never inlined: calls to the global allocator carry allocation semantics
// the optimiser reasons with (calls elided, moved to the stack, returned
// memory assumed fresh). Release builds with LTO, which could inline these
// bodies across crates, aborted on a failed small allocation, while builds
// without LTO did not; the exact interaction was not pinned down. Behind an
// opaque call the pools are only ever touched by the code below.
unsafe impl GlobalAlloc for Pooled {
    #[inline(never)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match class(layout) {
            Some(c) => {
                let owned = owned();
                // SAFETY: an owned pool is only touched by its owning thread;
                // either pool hands out blocks of the class size.
                if !owned.is_null() { return unsafe { (*owned).take(c) } }
                unsafe { shard().lock().unwrap_or_else(|e| e.into_inner()).take(c) }
            }
            // SAFETY: forwarded unchanged.
            None => unsafe { System.alloc(layout) },
        }
    }

    #[inline(never)]
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

    #[inline(never)]
    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        match class(layout) {
            Some(c) => {
                let owned = owned();
                // SAFETY: `block` came from `alloc` with this layout, so from a
                // pool; an owned pool is only touched by its owning thread.
                if !owned.is_null() { return unsafe { (*owned).give(c, block) } }
                unsafe { shard().lock().unwrap_or_else(|e| e.into_inner()).give(c, block) }
            }
            // SAFETY: forwarded unchanged.
            None => unsafe { System.dealloc(block, layout) },
        }
    }

    #[inline(never)]
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

    #[test]
    fn owned_pools_pass_between_workers_without_handing_out_live_blocks() {
        let l = layout(64, 8);
        // A block allocated under one worker's pool, still live after it ends.
        let kept = std::thread::spawn(move || {
            let _pool = own_pool();
            unsafe {
                let freed = Pooled.alloc(l);
                Pooled.dealloc(freed, l);
                let kept = Pooled.alloc(l);
                kept.write_bytes(7, 64);
                kept as usize
            }
        }).join().expect("first worker");
        // The next worker may take that pool over: none of its blocks overlap
        // the live one.
        std::thread::spawn(move || {
            let _pool = own_pool();
            unsafe {
                let blocks: Vec<*mut u8> = (0..64).map(|_| Pooled.alloc(l)).collect();
                for &block in &blocks {
                    assert_ne!(block as usize, kept);
                    block.write_bytes(1, 64);
                }
                assert!((0..64).all(|k| *(kept as *mut u8).add(k) == 7));
                for block in blocks { Pooled.dealloc(block, l) }
                Pooled.dealloc(kept as *mut u8, l);
            }
        }).join().expect("second worker");
    }
}
