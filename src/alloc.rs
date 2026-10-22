//! The binary's allocator: small blocks come from one-size pages owned by
//! pools, everything else from the system allocator.
//!
//! Linting allocates millions of short-lived small values (records, lists,
//! texts, tree vectors). On Windows the system allocator is one process heap
//! every worker thread contends on. A worker thread owns a pool outright for
//! as long as it holds [`own_pool`]'s guard: its allocations take no lock at
//! all. The guard hands the pool on to the next worker when it ends. Other
//! threads share mutex-guarded shards (handed out round-robin, so they rarely
//! share one).
//!
//! Pools take 64 KiB pages from one shared store; a page holds blocks of one
//! size. A page whose blocks are all free goes back to the store, where any
//! pool can take it for any size: memory one file's peak needed for small
//! records serves the next file's lists, instead of every worker keeping its
//! high-water mark for every size at once. A block freed by a thread that does
//! not hold its page's pool goes on the page's remote list; the owner takes
//! those back when the page runs dry and when it [`trim`]s.
//!
//! Pages are never returned to the system: the peak is what costs, and wasm
//! memory cannot shrink anyway.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::Mutex;

/// Blocks up to this size (in 16-byte classes) are pooled.
const SMALL: usize = 2048;
const QUANTUM: usize = 16;
const CLASSES: usize = SMALL / QUANTUM;
/// Pages are aligned to their size, so a block finds its page by masking.
const PAGE: usize = 64 * 1024;
/// Bytes the store takes from the system at a time.
const SEGMENT: usize = 4 * 1024 * 1024;
const SHARDS: usize = 64;

/// A page's header, at its start; blocks follow.
struct Page {
    /// Free blocks, linked through their first word (owner only).
    free: *mut u8,
    /// Start of the never-used tail (owner only).
    bump: *mut u8,
    /// Blocks freed by other threads, for the owner to take back.
    remote: AtomicPtr<u8>,
    /// The owning pool; fixed while any of the page's blocks is live.
    owner: AtomicPtr<Pool>,
    /// Blocks handed out and not yet taken back (owner only).
    used: usize,
    size: usize,
    /// On its class's full list rather than the available one.
    full: bool,
    prev: *mut Page,
    next: *mut Page,
}

const PAGE_HEADER: usize = std::mem::size_of::<Page>().next_multiple_of(QUANTUM);

impl Page {
    fn of(block: *mut u8) -> *mut Page {
        block.map_addr(|address| address & !(PAGE - 1)).cast()
    }

    /// Moves the remote frees onto the free list.
    ///
    /// # Safety
    /// Only the owning pool's holder may call this.
    unsafe fn collect(page: *mut Page) {
        // SAFETY: remote blocks are this page's, freed and linked by `give_remote`.
        unsafe {
            let mut block = (*page).remote.swap(null_mut(), Ordering::Acquire);
            while !block.is_null() {
                let next = *block.cast::<*mut u8>();
                *block.cast::<*mut u8>() = (*page).free;
                (*page).free = block;
                (*page).used -= 1;
                block = next;
            }
        }
    }

    /// A block from the page, or null when it has none left.
    ///
    /// # Safety
    /// Only the owning pool's holder may call this.
    unsafe fn pop(page: *mut Page) -> *mut u8 {
        // SAFETY: the free list and the tail lie inside the page.
        unsafe {
            if (*page).free.is_null() {
                let size = (*page).size;
                if page.cast::<u8>().add(PAGE).offset_from((*page).bump) as usize >= size {
                    let block = (*page).bump;
                    (*page).bump = block.add(size);
                    (*page).used += 1;
                    return block;
                }
                Page::collect(page);
                if (*page).free.is_null() { return null_mut() }
            }
            let block = (*page).free;
            (*page).free = *block.cast::<*mut u8>();
            (*page).used += 1;
            block
        }
    }

    /// Frees `block` from a thread that does not hold the page's pool.
    ///
    /// # Safety
    /// `block` is a live block of `page`.
    unsafe fn give_remote(page: *mut Page, block: *mut u8) {
        // SAFETY: the page outlives its live blocks; only the first word of
        // the freed block is written.
        unsafe {
            let remote = &(*page).remote;
            let mut head = remote.load(Ordering::Relaxed);
            loop {
                *block.cast::<*mut u8>() = head;
                match remote.compare_exchange_weak(head, block, Ordering::Release, Ordering::Relaxed) {
                    Ok(_) => return,
                    Err(current) => head = current,
                }
            }
        }
    }
}

/// Unused pages, linked through `next`.
struct Store(*mut Page);

// The store is only touched under its mutex.
unsafe impl Send for Store {}

static STORE: Mutex<Store> = Mutex::new(Store(null_mut()));

impl Store {
    fn lock() -> std::sync::MutexGuard<'static, Store> {
        STORE.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A page for `owner`'s blocks of `size`, or null when the system is out.
    unsafe fn take(owner: *mut Pool, size: usize) -> *mut Page {
        let mut page = {
            let mut store = Store::lock();
            let head = store.0;
            // SAFETY: stored pages keep their `next` link.
            if !head.is_null() { store.0 = unsafe { (*head).next } }
            head
        };
        if page.is_null() {
            // SAFETY: SEGMENT is a non-zero multiple of the PAGE alignment.
            let segment = unsafe { System.alloc(Layout::from_size_align_unchecked(SEGMENT, PAGE)) };
            if segment.is_null() { return null_mut() }
            let mut store = Store::lock();
            for index in 1..SEGMENT / PAGE {
                // SAFETY: each page lies inside the segment just allocated.
                unsafe {
                    let spare = segment.add(index * PAGE).cast::<Page>();
                    (&raw mut (*spare).next).write(store.0);
                    store.0 = spare;
                }
            }
            page = segment.cast();
        }
        // SAFETY: the page is unused memory of PAGE bytes, PAGE-aligned.
        unsafe {
            page.write(Page {
                free: null_mut(),
                bump: page.cast::<u8>().add(PAGE_HEADER),
                remote: AtomicPtr::new(null_mut()),
                owner: AtomicPtr::new(owner),
                used: 0,
                size,
                full: false,
                prev: null_mut(),
                next: null_mut(),
            });
        }
        page
    }

    /// Takes back a page none of whose blocks is live.
    unsafe fn give(page: *mut Page) {
        let mut store = Store::lock();
        // SAFETY: the page is no pool's any more.
        unsafe { (*page).next = store.0 };
        store.0 = page;
    }
}

/// A doubly linked list of one class's pages.
#[derive(Clone, Copy)]
struct Pages(*mut Page);

impl Pages {
    unsafe fn push(&mut self, page: *mut Page) {
        // SAFETY: both pages belong to this list's pool.
        unsafe {
            (*page).prev = null_mut();
            (*page).next = self.0;
            if !self.0.is_null() { (*self.0).prev = page }
        }
        self.0 = page;
    }

    /// Inserts `page` behind the head, so the page being allocated from stays first.
    unsafe fn push_second(&mut self, page: *mut Page) {
        if self.0.is_null() { return unsafe { self.push(page) } }
        // SAFETY: as `push`.
        unsafe {
            let next = (*self.0).next;
            (*page).prev = self.0;
            (*page).next = next;
            if !next.is_null() { (*next).prev = page }
            (*self.0).next = page;
        }
    }

    unsafe fn unlink(&mut self, page: *mut Page) {
        // SAFETY: `page` is on this list.
        unsafe {
            let (prev, next) = ((*page).prev, (*page).next);
            if prev.is_null() { self.0 = next } else { (*prev).next = next }
            if !next.is_null() { (*next).prev = prev }
        }
    }
}

#[derive(Clone, Copy)]
struct Class {
    /// Pages that may have room; the head is allocated from.
    available: Pages,
    /// Pages that ran out; their frees bring them back.
    full: Pages,
}

struct Pool {
    classes: [Class; CLASSES],
}

// Pools are only touched by their owning thread or under their mutex.
unsafe impl Send for Pool {}

impl Pool {
    const fn new() -> Self {
        Pool { classes: [Class { available: Pages(null_mut()), full: Pages(null_mut()) }; CLASSES] }
    }

    unsafe fn take(&mut self, class: usize) -> *mut u8 {
        let me: *mut Pool = self;
        let pages = &mut self.classes[class];
        loop {
            let page = pages.available.0;
            // SAFETY: this pool owns its listed pages.
            unsafe {
                if page.is_null() {
                    let page = Store::take(me, (class + 1) * QUANTUM);
                    if page.is_null() { return null_mut() }
                    pages.available.push(page);
                    continue;
                }
                let block = Page::pop(page);
                if !block.is_null() { return block }
                pages.available.unlink(page);
                (*page).full = true;
                pages.full.push(page);
            }
        }
    }

    /// Frees `block` of `page`, which this pool owns.
    unsafe fn give(&mut self, page: *mut Page, block: *mut u8) {
        // SAFETY: this pool owns `page`; `block` is one of its live blocks.
        unsafe {
            *block.cast::<*mut u8>() = (*page).free;
            (*page).free = block;
            (*page).used -= 1;
            let pages = &mut self.classes[(*page).size / QUANTUM - 1];
            if (*page).full {
                pages.full.unlink(page);
                (*page).full = false;
                pages.available.push_second(page);
            } else if (*page).used == 0 && pages.available.0 != page {
                pages.available.unlink(page);
                Store::give(page);
            }
        }
    }

    /// Takes back remote frees and hands empty pages to the store.
    unsafe fn trim(&mut self) {
        for pages in &mut self.classes {
            // SAFETY: this pool owns its listed pages.
            unsafe {
                let mut page = pages.full.0;
                while !page.is_null() {
                    let next = (*page).next;
                    Page::collect(page);
                    if !(*page).free.is_null() {
                        pages.full.unlink(page);
                        (*page).full = false;
                        if (*page).used == 0 { Store::give(page) } else { pages.available.push_second(page) }
                    }
                    page = next;
                }
                let mut page = pages.available.0;
                while !page.is_null() {
                    let next = (*page).next;
                    Page::collect(page);
                    if (*page).used == 0 {
                        pages.available.unlink(page);
                        Store::give(page);
                    }
                    page = next;
                }
            }
        }
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

/// Hands this thread's owned pool's empty pages back to the shared store
/// (call between files: what one file freed then serves any size).
pub fn trim() {
    let owned = owned();
    // SAFETY: an owned pool is only touched by its owning thread.
    if !owned.is_null() { unsafe { (*owned).trim() } }
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
        trim();
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

/// Heap accounting by owner, on when the process starts with
/// `ZLINT_HEAP_STATS` set (decided at the first allocation, so every block
/// is accounted or none is): every block carries a 16-byte header naming
/// the owner live at its allocation, so a free subtracts from that owner.
pub mod heap {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicIsize, AtomicU8, AtomicUsize, Ordering};

    /// Module work splits by phase: `roots` finds and loads a global name's
    /// candidate files, `catalog` builds program catalogs, `shared` keeps
    /// run-wide module answers; `modules` is the rest of module queries.
    pub const OWNERS: [&str; 10] = ["other", "units", "names", "semantic", "effects", "rules", "modules", "roots", "catalog", "shared"];
    pub const UNITS: usize = 1;
    pub const NAMES: usize = 2;
    pub const SEMANTIC: usize = 3;
    pub const EFFECTS: usize = 4;
    pub const RULES: usize = 5;
    pub const MODULES: usize = 6;
    pub const ROOTS: usize = 7;
    pub const CATALOG: usize = 8;
    pub const SHARED: usize = 9;
    /// 0 undecided, 1 off, 2 on.
    static MODE: AtomicU8 = AtomicU8::new(0);
    static LIVE: [AtomicIsize; OWNERS.len()] = [const { AtomicIsize::new(0) }; OWNERS.len()];
    static TOTAL: AtomicIsize = AtomicIsize::new(0);
    static PEAK: AtomicUsize = AtomicUsize::new(0);
    /// [`LIVE`] when [`PEAK`] was last raised.
    static AT_PEAK: [AtomicIsize; OWNERS.len()] = [const { AtomicIsize::new(0) }; OWNERS.len()];

    thread_local! {
        static OWNER: Cell<u8> = const { Cell::new(0) };
    }

    #[cfg(windows)]
    extern "system" {
        fn GetEnvironmentVariableA(name: *const u8, buffer: *mut u8, size: u32) -> u32;
    }
    #[cfg(not(windows))]
    extern "C" {
        fn getenv(name: *const std::ffi::c_char) -> *const std::ffi::c_char;
    }

    /// Reads the switch without allocating (std's env access would recurse
    /// into the allocator).
    fn requested() -> bool {
        // SAFETY: NUL-terminated names; Windows writes nothing for size 0.
        #[cfg(windows)]
        unsafe { GetEnvironmentVariableA(c"ZLINT_HEAP_STATS".as_ptr().cast(), std::ptr::null_mut(), 0) != 0 }
        #[cfg(not(windows))]
        unsafe { !getenv(c"ZLINT_HEAP_STATS".as_ptr()).is_null() }
    }

    pub fn on() -> bool {
        match MODE.load(Ordering::Relaxed) {
            0 => {
                let on = requested();
                MODE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
                on
            }
            mode => mode == 2,
        }
    }

    /// Allocations on this thread count against `owner` until it drops.
    pub struct Owner(u8);
    impl Owner {
        pub fn enter(owner: usize) -> Owner {
            Owner(OWNER.try_with(|current| current.replace(owner as u8)).unwrap_or(0))
        }
    }
    impl Drop for Owner {
        fn drop(&mut self) { let _ = OWNER.try_with(|current| current.set(self.0)); }
    }

    pub(super) fn current() -> u8 { OWNER.try_with(Cell::get).unwrap_or(0) }

    pub(super) fn add(owner: u8, bytes: isize) {
        LIVE[owner as usize].fetch_add(bytes, Ordering::Relaxed);
        let total = TOTAL.fetch_add(bytes, Ordering::Relaxed) + bytes;
        if bytes > 0 && total as usize > PEAK.load(Ordering::Relaxed) {
            PEAK.store(total as usize, Ordering::Relaxed);
            for (at, live) in AT_PEAK.iter().zip(&LIVE) { at.store(live.load(Ordering::Relaxed), Ordering::Relaxed) }
        }
    }

    /// `(owner, live bytes at the peak, live bytes now)` and the peak.
    pub fn report() -> (Vec<(&'static str, isize, isize)>, usize) {
        (OWNERS.iter().enumerate().map(|(index, &name)| (name, AT_PEAK[index].load(Ordering::Relaxed), LIVE[index].load(Ordering::Relaxed))).collect(),
            PEAK.load(Ordering::Relaxed))
    }
}

/// Header for accounted blocks: the owner and the requested size.
const HEADER: usize = 16;

pub struct Pooled;

impl Pooled {
    unsafe fn raw_alloc(&self, layout: Layout) -> *mut u8 {
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

    unsafe fn raw_dealloc(&self, block: *mut u8, layout: Layout) {
        match class(layout) {
            Some(_) => {
                let page = Page::of(block);
                // SAFETY: `block` came from `raw_alloc` with this layout, so
                // from a live page; a pool is only touched by its holder.
                unsafe {
                    let owner = (*page).owner.load(Ordering::Relaxed);
                    let owned = owned();
                    if !owned.is_null() {
                        if owner == owned { return (*owned).give(page, block) }
                        return Page::give_remote(page, block);
                    }
                    let mut shard = shard().lock().unwrap_or_else(|e| e.into_inner());
                    if owner == &raw mut *shard { shard.give(page, block) } else { Page::give_remote(page, block) }
                }
            }
            // SAFETY: forwarded unchanged.
            None => unsafe { System.dealloc(block, layout) },
        }
    }

    /// The layout an accounted block of `layout` takes (header first).
    fn accounted(layout: Layout) -> Option<Layout> {
        if layout.align() > HEADER { return None }
        Layout::from_size_align(layout.size() + HEADER, HEADER).ok()
    }
}

// Never inlined: calls to the global allocator carry allocation semantics
// the optimiser reasons with (calls elided, moved to the stack, returned
// memory assumed fresh). Release builds with LTO, which could inline these
// bodies across crates, aborted on a failed small allocation, while builds
// without LTO did not; the exact interaction was not pinned down. Behind an
// opaque call the pools are only ever touched by the code below.
unsafe impl GlobalAlloc for Pooled {
    #[inline(never)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if let Some(outer) = Self::accounted(layout).filter(|_| heap::on()) {
            // SAFETY: the header sits in front of the caller's bytes, inside
            // the block, which is HEADER-aligned.
            unsafe {
                let block = self.raw_alloc(outer);
                if block.is_null() { return block }
                let owner = heap::current();
                *block = owner;
                heap::add(owner, layout.size() as isize);
                return block.add(HEADER);
            }
        }
        // SAFETY: forwarded unchanged.
        unsafe { self.raw_alloc(layout) }
    }

    #[inline(never)]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if heap::on() || class(layout).is_some() {
            // SAFETY: as `alloc`; then zeroed over the requested size.
            let block = unsafe { self.alloc(layout) };
            if !block.is_null() { unsafe { block.write_bytes(0, layout.size()) } }
            return block;
        }
        // SAFETY: forwarded unchanged.
        unsafe { System.alloc_zeroed(layout) }
    }

    #[inline(never)]
    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        if let Some(outer) = Self::accounted(layout).filter(|_| heap::on()) {
            // SAFETY: accounted blocks start HEADER bytes before `block`.
            unsafe {
                let start = block.sub(HEADER);
                heap::add(*start, -(layout.size() as isize));
                return self.raw_dealloc(start, outer);
            }
        }
        // SAFETY: forwarded unchanged.
        unsafe { self.raw_dealloc(block, layout) }
    }

    #[inline(never)]
    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `layout.align()` is a valid alignment and the caller keeps
        // `new_size` within isize after rounding.
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        if heap::on() {
            // SAFETY: a fresh accounted block, filled from the old one.
            unsafe {
                let moved = self.alloc(new_layout);
                if !moved.is_null() {
                    std::ptr::copy_nonoverlapping(block, moved, layout.size().min(new_size));
                    self.dealloc(block, layout);
                }
                return moved;
            }
        }
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

    #[test]
    fn freed_pages_serve_other_sizes_whichever_thread_freed_their_blocks() {
        std::thread::spawn(|| {
            let _pool = own_pool();
            let small = layout(32, 8);
            let blocks: Vec<usize> = (0..4 * PAGE / 32).map(|_| unsafe { Pooled.alloc(small) } as usize).collect();
            let pages: std::collections::HashSet<usize> = blocks.iter().map(|&block| block & !(PAGE - 1)).collect();
            let (local, remote) = blocks.split_at(blocks.len() / 2);
            for &block in local { unsafe { Pooled.dealloc(block as *mut u8, small) } }
            let remote = remote.to_vec();
            std::thread::spawn(move || {
                for block in remote { unsafe { Pooled.dealloc(block as *mut u8, small) } }
            }).join().expect("freeing thread");
            trim();
            let large = layout(1024, 16);
            let reused: Vec<*mut u8> = (0..4 * PAGE / 1024).map(|_| unsafe { Pooled.alloc(large) }).collect();
            let landed = reused.iter().filter(|&&block| pages.contains(&(block as usize & !(PAGE - 1)))).count();
            for block in reused { unsafe { Pooled.dealloc(block, large) } }
            assert!(landed > 0, "no page freed by 32-byte blocks served 1024-byte blocks");
        }).join().expect("worker");
    }
}
