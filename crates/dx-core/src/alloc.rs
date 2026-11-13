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
//! A run's peak is what costs, so pages never leave the store. A long-lived
//! host (the language server) calls [`release`] once a burst of work ends:
//! the memory behind unused pages goes back to the system, and comes back
//! when a page is taken again (wasm memory cannot shrink: there it stays).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, UnsafeCell};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

/// The allocator's lock. std's `Mutex` cannot guard it: on macOS it boxes a
/// pthread mutex on first lock, and that allocation would lock again.
struct Lock<T> {
    held: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: the value is only reached through a guard, one holder at a time.
unsafe impl<T: Send> Sync for Lock<T> {}

impl<T> Lock<T> {
    const fn new(value: T) -> Lock<T> {
        Lock { held: AtomicBool::new(false), value: UnsafeCell::new(value) }
    }

    fn lock(&self) -> Held<'_, T> {
        let mut spins = 0u32;
        while self.held.swap(true, Ordering::Acquire) {
            while self.held.load(Ordering::Relaxed) {
                spins += 1;
                if spins < 64 { std::hint::spin_loop() } else { std::thread::yield_now() }
            }
        }
        Held(self)
    }
}

struct Held<'a, T>(&'a Lock<T>);

impl<T> std::ops::Deref for Held<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: this guard holds the lock.
        unsafe { &*self.0.value.get() }
    }
}

impl<T> std::ops::DerefMut for Held<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: this guard holds the lock.
        unsafe { &mut *self.0.value.get() }
    }
}

impl<T> Drop for Held<'_, T> {
    fn drop(&mut self) {
        self.0.held.store(false, Ordering::Release);
    }
}

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

/// Unused pages, linked through `next`: those whose memory is still the
/// process's, and those [`release`]d to the system.
struct Store {
    kept: *mut Page,
    released: *mut Page,
    /// Pages on each list, and segments taken from the system.
    kept_pages: usize,
    released_pages: usize,
    segments: usize,
}

// The store is only touched under its mutex.
unsafe impl Send for Store {}

static STORE: Lock<Store> = Lock::new(Store { kept: null_mut(), released: null_mut(), kept_pages: 0, released_pages: 0, segments: 0 });
/// Each store page's first system page holds its link: it is never released,
/// and releases must start on a system page (16 KiB on Apple Silicon).
#[cfg(target_vendor = "apple")]
const LINK: usize = 16 * 1024;
#[cfg(not(target_vendor = "apple"))]
const LINK: usize = 4 * 1024;

impl Store {
    fn lock() -> Held<'static, Store> {
        STORE.lock()
    }

    /// Pops the head of `list`.
    ///
    /// # Safety
    /// The store's lock is held; listed pages keep their `next` link.
    unsafe fn pop(list: &mut *mut Page) -> *mut Page {
        let head = *list;
        if !head.is_null() { *list = unsafe { (*head).next } }
        head
    }

    /// A page for `owner`'s blocks of `size`, or null when the system is out.
    unsafe fn take(owner: *mut Pool, size: usize) -> *mut Page {
        let (mut page, released) = {
            let mut store = Store::lock();
            // SAFETY: the lock is held.
            match unsafe { Store::pop(&mut store.kept) } {
                page if !page.is_null() => {
                    store.kept_pages -= 1;
                    (page, false)
                }
                _ => {
                    let page = unsafe { Store::pop(&mut store.released) };
                    if !page.is_null() { store.released_pages -= 1 }
                    (page, true)
                }
            }
        };
        // SAFETY: a released page's memory past its link is the system's.
        if !page.is_null() && released && !unsafe { system::reclaim(page.cast::<u8>().add(LINK), PAGE - LINK) } {
            return null_mut();
        }
        if page.is_null() {
            // SAFETY: SEGMENT is a non-zero multiple of PAGE.
            let segment = unsafe { system::segment() };
            if segment.is_null() { return null_mut() }
            let mut store = Store::lock();
            store.segments += 1;
            store.kept_pages += SEGMENT / PAGE - 1;
            for index in 1..SEGMENT / PAGE {
                // SAFETY: each page lies inside the segment just allocated.
                unsafe {
                    let spare = segment.add(index * PAGE).cast::<Page>();
                    (&raw mut (*spare).next).write(store.kept);
                    store.kept = spare;
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
        unsafe { (*page).next = store.kept };
        store.kept = page;
        store.kept_pages += 1;
    }
}

/// Hands the memory of every unused page back to the system; their
/// addresses stay the store's, so later allocations take them again. Call it
/// when a burst of work has ended. Pools no thread owns (parked, lasting,
/// shards) first take back what other threads freed, so pages they emptied
/// go too; owned pools [`trim`] into the store when their workers finish.
pub fn release() {
    // SAFETY: each pool is held (parked ones through their list's lock).
    unsafe {
        for parked in &PARKED {
            for pool in parked.lock().iter() { (*pool.0).trim() }
        }
        LASTING.lock().trim();
        for shard in &POOLS { shard.lock().trim() }
    }
    system::compact();
    let (head, count) = {
        let mut store = Store::lock();
        (std::mem::replace(&mut store.kept, null_mut()), std::mem::take(&mut store.kept_pages))
    };
    if head.is_null() { return }
    // SAFETY: the detached pages are no pool's and no other thread sees them;
    // each keeps its link page, so the list stays intact.
    let mut tail = head;
    unsafe {
        loop {
            system::release(tail.cast::<u8>().add(LINK), PAGE - LINK);
            if (*tail).next.is_null() { break }
            tail = (*tail).next;
        }
    }
    let mut store = Store::lock();
    // SAFETY: the lock is held; `tail` ends the detached list.
    unsafe { (*tail).next = store.released };
    store.released = head;
    store.released_pages += count;
}
/// Segments taken from the system, and the store's kept and released pages.
fn store_pages() -> (usize, usize, usize) {
    let store = Store::lock();
    (store.segments, store.kept_pages, store.released_pages)
}

/// (pages, bytes handed out) of the pools no thread owns right now: parked
/// work and editor pools, the lasting pool and the shards.
fn pool_usage() -> [(&'static str, usize, usize); 4] {
    let parked = |family: Family| PARKED[family as usize].lock().iter()
        // SAFETY: parked pools belong to no thread; the parked list's lock is held.
        .map(|pool| unsafe { (*pool.0).usage() }).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    // SAFETY: each pool is held while it is read.
    let lasting = unsafe { LASTING.lock().usage() };
    let shards = POOLS.iter().map(|pool| unsafe { pool.lock().usage() }).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    let (work, editor) = (parked(Family::Work), parked(Family::Editor));
    [("work", work.0, work.1), ("editor", editor.0, editor.1), ("lasting", lasting.0, lasting.1), ("shards", shards.0, shards.1)]
}

/// Per block size, across the pools no thread owns: (size, pages, bytes handed out).
fn class_usage() -> Vec<(usize, usize, usize)> {
    let mut rows: Vec<(usize, usize, usize)> = (0..CLASSES).map(|class| ((class + 1) * QUANTUM, 0, 0)).collect();
    let mut count = |pool: &Pool| {
        for (row, class) in rows.iter_mut().zip(&pool.classes) {
            for list in [class.available, class.full] {
                let mut page = list.0;
                // SAFETY: the caller holds the pool; listed pages are its.
                while !page.is_null() {
                    unsafe {
                        row.1 += 1;
                        row.2 += (*page).used * (*page).size;
                        page = (*page).next;
                    }
                }
            }
        }
    };
    // SAFETY: parked pools belong to no thread; the rest are held by their lock.
    unsafe { for parked in &PARKED { for pool in parked.lock().iter() { count(&*pool.0) } } }
    count(&LASTING.lock());
    for shard in &POOLS { count(&shard.lock()) }
    rows
}

/// The system memory behind the store.
mod system {
    use super::{PAGE, SEGMENT};

    #[cfg(windows)]
    mod os {
        use std::ffi::c_void;
        extern "system" {
            fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
            fn VirtualFree(address: *mut c_void, size: usize, kind: u32) -> i32;
        }
        const MEM_COMMIT: u32 = 0x1000;
        const MEM_RESERVE: u32 = 0x2000;
        const MEM_DECOMMIT: u32 = 0x4000;
        const PAGE_READWRITE: u32 = 0x04;

        /// Fresh committed memory, aligned to the 64 KiB allocation granularity.
        pub unsafe fn segment(size: usize) -> *mut u8 {
            let _ = (MEM_RESERVE, VirtualAlloc as unsafe extern "system" fn(_, _, _, _) -> _);
            unsafe { std::alloc::GlobalAlloc::alloc(&std::alloc::System, std::alloc::Layout::from_size_align_unchecked(size, super::super::PAGE)) }
        }

        pub unsafe fn release(at: *mut u8, size: usize) {
            unsafe { VirtualFree(at.cast(), size, MEM_DECOMMIT) };
        }

        pub unsafe fn reclaim(at: *mut u8, size: usize) -> bool {
            !unsafe { VirtualAlloc(at.cast(), size, MEM_COMMIT, PAGE_READWRITE) }.is_null()
        }

        /// The process heap (large blocks) gives its free ranges back.
        pub fn compact() {
            extern "system" {
                fn GetProcessHeap() -> *mut c_void;
                fn HeapCompact(heap: *mut c_void, flags: u32) -> usize;
            }
            // SAFETY: compacts the process's own heap; no block moves.
            unsafe { HeapCompact(GetProcessHeap(), 0) };
        }
    }

    #[cfg(not(windows))]
    mod os {
        use std::alloc::{GlobalAlloc, Layout, System};

        pub unsafe fn segment(size: usize) -> *mut u8 {
            unsafe { System.alloc(Layout::from_size_align_unchecked(size, super::PAGE)) }
        }

        #[cfg(unix)]
        pub unsafe fn release(at: *mut u8, size: usize) {
            extern "C" {
                fn madvise(address: *mut std::ffi::c_void, size: usize, advice: i32) -> i32;
            }
            // macOS keeps MADV_DONTNEED pages resident; MADV_FREE frees them.
            #[cfg(target_vendor = "apple")]
            const ADVICE: i32 = 5;
            #[cfg(not(target_vendor = "apple"))]
            const ADVICE: i32 = 4;
            unsafe { madvise(at.cast(), size, ADVICE) };
        }

        #[cfg(not(unix))]
        pub unsafe fn release(_: *mut u8, _: usize) {}

        /// Released memory is the process's again on its next touch.
        pub unsafe fn reclaim(_: *mut u8, _: usize) -> bool {
            true
        }

        pub fn compact() {}
    }

    /// SEGMENT fresh bytes, PAGE-aligned; null when the system is out.
    pub unsafe fn segment() -> *mut u8 {
        unsafe { os::segment(SEGMENT) }
    }

    /// Hands `size` bytes at `at` (system-page aligned) back to the system.
    pub unsafe fn release(at: *mut u8, size: usize) {
        unsafe { os::release(at, size) }
    }

    /// Makes released bytes usable again; false when the system is out.
    pub unsafe fn reclaim(at: *mut u8, size: usize) -> bool {
        unsafe { os::reclaim(at, size) }
    }

    /// Asks the system allocator to give its free memory back.
    pub fn compact() {
        os::compact()
    }

    const _: () = assert!(PAGE.is_multiple_of(super::LINK) && SEGMENT.is_multiple_of(PAGE));
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

    /// Takes over `other`'s pages; `other` ends with none.
    ///
    /// # Safety
    /// Both pools are held by the caller, and no thread owns `other`'s
    /// pages any more (frees from any thread go on their remote lists).
    unsafe fn absorb(&mut self, other: &mut Pool) {
        let me: *mut Pool = self;
        for (mine, theirs) in self.classes.iter_mut().zip(&mut other.classes) {
            for (into, from) in [(&mut mine.available, &mut theirs.available), (&mut mine.full, &mut theirs.full)] {
                // SAFETY: `from` lists pages of `other`, which the caller holds.
                unsafe {
                    while !from.0.is_null() {
                        let page = from.0;
                        from.unlink(page);
                        (*page).owner.store(me, Ordering::Relaxed);
                        into.push_second(page);
                    }
                }
            }
        }
    }

    /// Pages listed, and bytes of blocks handed out from them.
    ///
    /// # Safety
    /// The caller holds this pool.
    unsafe fn usage(&self) -> (usize, usize) {
        let (mut pages, mut used) = (0, 0);
        for class in &self.classes {
            for list in [class.available, class.full] {
                let mut page = list.0;
                // SAFETY: listed pages are this pool's.
                while !page.is_null() {
                    unsafe {
                        pages += 1;
                        used += (*page).used * (*page).size;
                        page = (*page).next;
                    }
                }
            }
        }
        (pages, used)
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

static POOLS: [Lock<Pool>; SHARDS] = [const { Lock::new(Pool::new()) }; SHARDS];

/// The pool for what a long-lived host keeps ([`lasting`]), and its address
/// once used, so frees find it without locking.
static LASTING: Lock<Pool> = Lock::new(Pool::new());
static LASTING_POOL: AtomicPtr<Pool> = AtomicPtr::new(null_mut());

/// Small allocations made inside `f` come from the lasting pool: data a
/// host keeps past the work that made it (per-file reads, interned paths)
/// then fills pages of its own, and the work's pages empty out wholly when
/// it ends, for [`release`] to hand back.
pub fn lasting<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) { let _ = LASTING_SCOPE.try_with(|scope| scope.set(self.0)); }
    }
    let _restore = Restore(LASTING_SCOPE.with(|scope| scope.replace(true)));
    f()
}
static NEXT_SHARD: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// This thread's shard, plus one (zero: not yet chosen).
    static SHARD: Cell<usize> = const { Cell::new(0) };
    /// The pool this thread owns ([`own_pool`]), or null.
    static OWNED: Cell<*mut Pool> = const { Cell::new(null_mut()) };
    /// Inside [`lasting`].
    static LASTING_SCOPE: Cell<bool> = const { Cell::new(false) };
}

fn in_lasting() -> bool {
    LASTING_SCOPE.try_with(Cell::get).unwrap_or(false)
}

fn lasting_pool() -> Held<'static, Pool> {
    let mut pool = LASTING.lock();
    LASTING_POOL.store(&raw mut *pool, Ordering::Relaxed);
    pool
}

struct Parked(*mut Pool);

// Parked pools belong to no thread until one takes them.
unsafe impl Send for Parked {}

/// Which workers share parked pools. A language server's editor keeps its
/// last run's answers between keystrokes: pages shared with background
/// work would end up holding a few of those long-lived blocks each, and
/// none of them could go back to the system.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    Work,
    Editor,
}

/// Pools whose owners finished, kept for the next worker of their family.
static PARKED: [Lock<Vec<Parked>>; 2] = [const { Lock::new(Vec::new()) }; 2];

thread_local! {
    static FAMILY: Cell<Family> = const { Cell::new(Family::Work) };
}

/// The family of this thread's owned pool, for workers it starts.
pub fn family() -> Family {
    FAMILY.try_with(Cell::get).unwrap_or(Family::Work)
}

/// Gives this thread a pool of its own until the guard drops; small
/// allocations on it then take no lock.
pub fn own_pool() -> OwnedPool {
    own_pool_in(Family::Work)
}

/// [`own_pool`] from `family`'s parked pools.
pub fn own_pool_in(family: Family) -> OwnedPool {
    let parked = PARKED[family as usize].lock().pop();
    let pool = parked.map_or_else(|| Box::into_raw(Box::new(Pool::new())), |parked| parked.0);
    let previous = OWNED.with(|owned| owned.replace(pool));
    let previous_family = FAMILY.with(|current| current.replace(family));
    OwnedPool { pool, previous, family, previous_family, thread: std::marker::PhantomData }
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
    family: Family,
    previous_family: Family,
    /// Ownership stays on the thread that took it.
    thread: std::marker::PhantomData<*mut Pool>,
}

impl Drop for OwnedPool {
    /// Parks the pool, folded into one already parked: the free blocks its
    /// partly used pages hold serve the next worker, instead of waiting in
    /// a pool no worker may take again (a scan's many workers, then one).
    fn drop(&mut self) {
        trim();
        OWNED.with(|owned| owned.set(self.previous));
        FAMILY.with(|current| current.set(self.previous_family));
        let mut parked = PARKED[self.family as usize].lock();
        match parked.first() {
            // SAFETY: parked pools belong to no thread; this one is no
            // thread's any more, and is dropped once emptied.
            Some(into) => unsafe {
                (*into.0).absorb(&mut *self.pool);
                drop(Box::from_raw(self.pool));
            },
            None => parked.push(Parked(self.pool)),
        }
    }
}

/// This thread's owned pool, or null.
fn owned() -> *mut Pool {
    OWNED.try_with(Cell::get).unwrap_or(null_mut())
}

fn shard() -> &'static Lock<Pool> {
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
/// `DXLINT_HEAP_STATS` set (decided at the first allocation, so every block
/// is accounted or none is): every block carries a 16-byte header naming
/// the owner live at its allocation, so a free subtracts from that owner.
pub mod heap {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicIsize, AtomicU8, AtomicUsize, Ordering};

    /// Module work splits by phase: `roots` finds and loads a global name's
    /// candidate files, `catalog` builds program catalogs, `shared` keeps
    /// run-wide module answers; `modules` is the rest of module queries.
    pub const OWNERS: [&str; 13] = ["other", "units", "names", "semantic", "effects", "rules", "modules", "roots", "catalog", "shared", "kept reads", "file versions", "part reads"];
    pub const UNITS: usize = 1;
    pub const NAMES: usize = 2;
    pub const SEMANTIC: usize = 3;
    pub const EFFECTS: usize = 4;
    pub const RULES: usize = 5;
    pub const MODULES: usize = 6;
    pub const ROOTS: usize = 7;
    pub const CATALOG: usize = 8;
    pub const SHARED: usize = 9;
    /// Reads a long-lived host keeps per linted file (`reads::kept`).
    pub const KEPT: usize = 10;
    /// Versions of other files those reads name (`parts::FileParts`).
    pub const FILE_VERSIONS: usize = 11;
    /// What the reads read of each version (`parts::PartReads`).
    pub const PART_READS: usize = 12;
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

    /// Reads a switch without allocating (std's env access would recurse
    /// into the allocator).
    fn requested(name: &std::ffi::CStr) -> bool {
        // SAFETY: NUL-terminated names; Windows writes nothing for size 0.
        #[cfg(windows)]
        unsafe { GetEnvironmentVariableA(name.as_ptr().cast(), std::ptr::null_mut(), 0) != 0 }
        #[cfg(not(windows))]
        unsafe { !getenv(name.as_ptr()).is_null() }
    }

    /// `mode` decided once from the switch `name`.
    fn switch(mode: &AtomicU8, name: &std::ffi::CStr) -> bool {
        match mode.load(Ordering::Relaxed) {
            0 => {
                let on = requested(name);
                mode.store(if on { 2 } else { 1 }, Ordering::Relaxed);
                on
            }
            mode => mode == 2,
        }
    }

    pub fn on() -> bool { switch(&MODE, c"DXLINT_HEAP_STATS") }

    /// Large blocks by the call stack that took them, on with
    /// `DXLINT_HEAP_SITES` as well (Windows): the stack is captured without
    /// allocating into a fixed table, and a block's header keeps its site
    /// so a free subtracts from it.
    pub(super) mod sites {
        use std::sync::atomic::{AtomicIsize, AtomicU32, AtomicU64, AtomicU8, Ordering};

        /// Recursion gives one allocation many stacks: room for all of them.
        const SLOTS: usize = 65536;
        const DEPTH: usize = 20;
        const PROBES: usize = 256;
        static MODE: AtomicU8 = AtomicU8::new(0);
        /// Bytes of blocks whose site found no free slot.
        static UNPLACED: AtomicIsize = AtomicIsize::new(0);

        struct Site {
            /// The stack's hash (never 0) once claimed.
            hash: AtomicU32,
            ready: AtomicU8,
            frames: [AtomicU64; DEPTH],
            live: AtomicIsize,
            most: AtomicIsize,
        }

        static SITES: [Site; SLOTS] = [const { Site {
            hash: AtomicU32::new(0), ready: AtomicU8::new(0), frames: [const { AtomicU64::new(0) }; DEPTH],
            live: AtomicIsize::new(0), most: AtomicIsize::new(0),
        } }; SLOTS];

        #[cfg(windows)]
        extern "system" {
            fn RtlCaptureStackBackTrace(skip: u32, count: u32, frames: *mut u64, hash: *mut u32) -> u16;
        }

        pub fn on() -> bool { cfg!(windows) && super::switch(&MODE, c"DXLINT_HEAP_SITES") }

        /// The caller's stack and its hash (never 0), without allocating.
        #[cfg(windows)]
        fn capture() -> Option<([u64; DEPTH], u32)> {
            let mut frames = [0u64; DEPTH];
            let mut hash = 0u32;
            // SAFETY: room for DEPTH frames.
            (unsafe { RtlCaptureStackBackTrace(2, DEPTH as u32, frames.as_mut_ptr(), &mut hash) } != 0).then_some((frames, hash.max(1)))
        }

        #[cfg(not(windows))]
        fn capture() -> Option<([u64; DEPTH], u32)> { None }

        /// Counts `bytes` taken here; the site's slot + 1 (0: none).
        pub fn taken(bytes: isize) -> u32 {
            if !on() { return 0 }
            let Some((frames, hash)) = capture() else {
                UNPLACED.fetch_add(bytes, Ordering::Relaxed);
                return 0;
            };
            let mut at = hash as usize % SLOTS;
            for _ in 0..PROBES {
                let site = &SITES[at];
                match site.hash.compare_exchange(0, hash, Ordering::AcqRel, Ordering::Acquire) {
                    Ok(_) => {
                        for (slot, frame) in site.frames.iter().zip(frames) { slot.store(frame, Ordering::Relaxed) }
                        site.ready.store(1, Ordering::Release);
                    }
                    Err(claimed) if claimed == hash => {}
                    Err(_) => {
                        at = (at + 1) % SLOTS;
                        continue;
                    }
                }
                let live = site.live.fetch_add(bytes, Ordering::Relaxed) + bytes;
                site.most.fetch_max(live, Ordering::Relaxed);
                return at as u32 + 1;
            }
            UNPLACED.fetch_add(bytes, Ordering::Relaxed);
            0
        }

        pub fn given(slot: u32, bytes: isize) {
            if !on() { return }
            match slot.checked_sub(1) {
                Some(at) => SITES[at as usize].live.fetch_sub(bytes, Ordering::Relaxed),
                None => UNPLACED.fetch_sub(bytes, Ordering::Relaxed),
            };
        }

        /// Large blocks by the innermost frames that took them outside the
        /// allocator and std's collections (stacks that differ only further
        /// out add up): what is live now, and the sum of each stack's most
        /// live at once (stacks peak at different times, so an upper bound).
        #[cfg(windows)]
        pub fn report() -> Option<String> {
            if !on() { return None }
            const SHOWN: usize = 25;
            // `DXLINT_HEAP_SITE_FRAMES=<n>`: frames kept per stack (default 5).
            let frames = std::env::var("DXLINT_HEAP_SITE_FRAMES").ok().and_then(|n| n.parse().ok()).unwrap_or(5);
            let mib = |bytes: isize| bytes as f64 / 1048576.0;
            let mut symbols = crate::sampler::Symbols::new();
            let plumbing = ["dxlint::_::", "dx_core::alloc::", "alloc::", "core::", "hashbrown::", "std::", "__rust", "smallvec::"];
            let mut groups: std::collections::HashMap<String, (isize, isize)> = std::collections::HashMap::new();
            for site in SITES.iter().filter(|site| site.ready.load(Ordering::Acquire) == 1) {
                let names: Vec<String> = site.frames.iter().map(|frame| frame.load(Ordering::Relaxed)).take_while(|&frame| frame != 0)
                    .map(|frame| symbols.name(frame - 1).to_string())
                    .skip_while(|name| plumbing.iter().any(|part| name.starts_with(part)))
                    .map(|name| name.split('<').next().unwrap_or_default().to_owned())
                    .take(frames).collect();
                let group = groups.entry(names.join(" < ")).or_default();
                group.0 += site.live.load(Ordering::Relaxed);
                group.1 += site.most.load(Ordering::Relaxed);
            }
            let mut rows: Vec<(String, (isize, isize))> = groups.into_iter().collect();
            let mut out = format!("large blocks by site, MiB (unplaced live {:.1})", mib(UNPLACED.load(Ordering::Relaxed)));
            for (title, most) in [("live now", false), ("most at once (summed)", true)] {
                rows.sort_by_key(|(_, (live, peak))| std::cmp::Reverse(if most { *peak } else { *live }));
                out.push_str(&format!("\n{title}:"));
                for (stack, (live, peak)) in rows.iter().take(SHOWN) {
                    out.push_str(&format!("\n{:>8.1} {:>8.1}  {stack}", mib(if most { *peak } else { *live }), mib(if most { *live } else { *peak })));
                }
            }
            Some(out)
        }

        #[cfg(not(windows))]
        pub fn report() -> Option<String> { None }
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
        STRETCH_PEAK.fetch_max(total, Ordering::Relaxed);
        if bytes > 0 && total as usize > PEAK.load(Ordering::Relaxed) {
            PEAK.store(total as usize, Ordering::Relaxed);
            for (at, live) in AT_PEAK.iter().zip(&LIVE) { at.store(live.load(Ordering::Relaxed), Ordering::Relaxed) }
        }
    }

    static STRETCH_PEAK: AtomicIsize = AtomicIsize::new(0);
    static FILES_MODE: AtomicU8 = AtomicU8::new(0);

    /// `DXLINT_HEAP_FILES` as well: each linted file prints the most and the
    /// final live heap across it.
    pub fn files_on() -> bool { on() && switch(&FILES_MODE, c"DXLINT_HEAP_FILES") }

    /// Live bytes now, and the most live since the last call.
    pub fn stretch() -> (isize, isize) {
        let now = TOTAL.load(Ordering::Relaxed);
        (now, STRETCH_PEAK.swap(now, Ordering::Relaxed))
    }

    /// `(owner, live bytes at the peak, live bytes now)` and the peak.
    pub fn report() -> (Vec<(&'static str, isize, isize)>, usize) {
        (OWNERS.iter().enumerate().map(|(index, &name)| (name, AT_PEAK[index].load(Ordering::Relaxed), LIVE[index].load(Ordering::Relaxed))).collect(),
            PEAK.load(Ordering::Relaxed))
    }

    /// Bytes of pooled blocks (class sizes, headers included) and of system
    /// blocks, live now.
    static POOLED: AtomicIsize = AtomicIsize::new(0);
    static LARGE: AtomicIsize = AtomicIsize::new(0);

    /// An accounted block of `layout` was taken (`sign` 1) or given back (-1).
    pub(super) fn taken(layout: std::alloc::Layout, sign: isize) {
        match super::class(layout) {
            Some(class) => POOLED.fetch_add(sign * ((class + 1) * super::QUANTUM) as isize, Ordering::Relaxed),
            None => LARGE.fetch_add(sign * layout.size() as isize, Ordering::Relaxed),
        };
    }

    /// [`report`] as one line, when accounting is on, with where the
    /// allocator's memory is now.
    pub fn summary() -> Option<String> {
        if !on() { return None }
        let (owners, peak) = report();
        let mib = |bytes: isize| bytes as f64 / 1048576.0;
        let rows: Vec<String> = owners.iter().map(|(name, at_peak, now)| format!("{name} {:.1} ({:.1} at end)", mib(*at_peak), mib(*now))).collect();
        let (segments, kept, released) = super::store_pages();
        let page = |pages: usize| mib((pages * super::PAGE) as isize);
        let pooled_pages = segments * (super::SEGMENT / super::PAGE) - kept - released;
        let pools: Vec<String> = super::pool_usage().iter()
            .map(|(name, pages, used)| format!("{name} {:.1} ({:.1} used)", page(*pages), mib(*used as isize))).collect();
        let mut summary = format!(
            "heap at peak {:.1} MiB by owner: {}\nallocator now: pools hold {:.1} MiB of pages ({:.1} MiB in live blocks; unowned pools: {}), store keeps {:.1} MiB, released {:.1} MiB ({:.1} MiB of it still committed), large blocks {:.1} MiB",
            mib(peak as isize), rows.join(", "), page(pooled_pages), mib(POOLED.load(Ordering::Relaxed)), pools.join(", "), page(kept), page(released),
            mib((released * super::LINK) as isize), mib(LARGE.load(Ordering::Relaxed)),
        );
        let mut classes = super::class_usage();
        classes.sort_by_key(|&(_, pages, used)| std::cmp::Reverse(pages * super::PAGE - used));
        let rows: Vec<String> = classes.iter().take(16)
            .map(|&(size, pages, used)| format!("{size}: {:.1} of {:.1}", mib(used as isize), page(pages))).collect();
        summary.push_str(&format!("\nblock sizes with the most unused room (used of pages, MiB): {}", rows.join(", ")));
        if let Some(sites) = sites::report() {
            summary.push('\n');
            summary.push_str(&sites);
        }
        Some(summary)
    }
}

/// Header for accounted blocks: the owner (first byte) and, for large
/// blocks, their site's slot ([`heap::sites`]) at [`SITE`].
const HEADER: usize = 16;
const SITE: usize = 4;

pub struct Pooled;

impl Pooled {
    unsafe fn raw_alloc(&self, layout: Layout) -> *mut u8 {
        match class(layout) {
            Some(c) => {
                // SAFETY: the lasting pool is only touched under its lock.
                if in_lasting() { return unsafe { lasting_pool().take(c) } }
                let owned = owned();
                // SAFETY: an owned pool is only touched by its owning thread;
                // either pool hands out blocks of the class size.
                if !owned.is_null() { return unsafe { (*owned).take(c) } }
                unsafe { shard().lock().take(c) }
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
                    if owner == LASTING_POOL.load(Ordering::Relaxed) { return lasting_pool().give(page, block) }
                    let owned = owned();
                    if !owned.is_null() {
                        if owner == owned { return (*owned).give(page, block) }
                        return Page::give_remote(page, block);
                    }
                    let mut shard = shard().lock();
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
        #[cfg(feature = "work")]
        crate::work::alloc();
        if let Some(outer) = Self::accounted(layout).filter(|_| heap::on()) {
            // SAFETY: the header sits in front of the caller's bytes, inside
            // the block, which is HEADER-aligned.
            unsafe {
                let block = self.raw_alloc(outer);
                if block.is_null() { return block }
                let owner = heap::current();
                *block = owner;
                if class(outer).is_none() { *block.add(SITE).cast::<u32>() = heap::sites::taken(layout.size() as isize) }
                heap::add(owner, layout.size() as isize);
                heap::taken(outer, 1);
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
                if class(outer).is_none() { heap::sites::given(*start.add(SITE).cast::<u32>(), layout.size() as isize) }
                heap::add(*start, -(layout.size() as isize));
                heap::taken(outer, -1);
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

    /// Tests that watch which pages the shared store hands out run apart.
    static STORE_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn freed_pages_serve_other_sizes_whichever_thread_freed_their_blocks() {
        let _apart = STORE_TESTS.lock().unwrap_or_else(|e| e.into_inner());
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

    #[test]
    fn pages_emptied_by_other_threads_after_their_worker_ended_go_on_release() {
        let _apart = STORE_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let l = layout(256, 16);
        // A worker's blocks outlive it (an editor's kept answers)...
        let blocks: Vec<usize> = std::thread::spawn(move || {
            let _pool = own_pool_in(Family::Editor);
            (0..4 * PAGE / 256).map(|_| unsafe { Pooled.alloc(l) } as usize).collect()
        }).join().expect("worker");
        let pages: std::collections::HashSet<usize> = blocks.iter().map(|&block| block & !(PAGE - 1)).collect();
        // ...and are freed later by another thread, onto their pages' remote lists.
        for &block in &blocks { unsafe { Pooled.dealloc(block as *mut u8, l) } }
        release();
        let still_listed = PARKED.iter().map(|parked| {
            let parked = parked.lock();
            parked.iter().flat_map(|pool| unsafe { &(*pool.0).classes }.iter().flat_map(|class| [class.available.0, class.full.0]))
                .flat_map(|mut page| std::iter::from_fn(move || {
                    if page.is_null() { return None }
                    let here = page;
                    page = unsafe { (*page).next };
                    Some(here)
                }))
                .filter(|&page| pages.contains(&(page as usize)))
                .count()
        }).sum::<usize>();
        // (Another worker may take a released page again, but then it is its own.)
        assert_eq!(still_listed, 0, "pages whose blocks were all freed stayed in parked pools");
    }

    #[test]
    fn released_pages_serve_allocations_again() {
        let _apart = STORE_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        std::thread::spawn(|| {
            let _pool = own_pool();
            let l = layout(512, 16);
            let count = 8 * PAGE / 512;
            let blocks: Vec<*mut u8> = (0..count).map(|_| unsafe { Pooled.alloc(l) }).collect();
            for block in blocks { unsafe { Pooled.dealloc(block, l) } }
            trim();
            release();
            // Whole pages of fresh blocks, written through and read back.
            let blocks: Vec<*mut u8> = (0..count).map(|_| unsafe { Pooled.alloc(l) }).collect();
            for (i, &block) in blocks.iter().enumerate() { unsafe { block.write_bytes(i as u8, 512) } }
            for (i, &block) in blocks.iter().enumerate() {
                assert!((0..512).all(|k| unsafe { *block.add(k) } == i as u8));
            }
            for block in blocks { unsafe { Pooled.dealloc(block, l) } }
        }).join().expect("worker");
    }
}
