//! A CPU sampling profiler that needs no elevated rights: with
//! `ZLINT_SAMPLE=<report path>` set, a thread suspends every other thread of
//! the process about once a millisecond and unwinds its call stack; at exit
//! the addresses are named through the executable's PDB (build with debug
//! line tables for names) and the report lists functions by self and
//! inclusive samples, waiting threads left out. Windows only; elsewhere
//! [`start_from_env`] does nothing.

#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    type Handle = *mut c_void;
    const DEPTH: usize = 48;
    const CAPACITY: usize = 250_000;
    const MAX_THREADS: usize = 128;
    const TH32CS_SNAPTHREAD: u32 = 0x4;
    /// Suspend/resume, get context, query information.
    const THREAD_ACCESS: u32 = 0x0002 | 0x0008 | 0x0040;
    /// CONTEXT_AMD64 | CONTEXT_CONTROL | CONTEXT_INTEGER | CONTEXT_FLOATING_POINT.
    const CONTEXT_FLAGS: u32 = 0x0010_0000 | 0x1 | 0x2 | 0x8;
    const OFFSET_FLAGS: usize = 0x30;
    const OFFSET_RSP: usize = 0x98;
    const OFFSET_RIP: usize = 0xF8;
    /// SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS.
    const SYMBOL_OPTIONS: u32 = 0x2 | 0x4;

    #[repr(C)]
    struct ThreadEntry {
        size: u32,
        usage: u32,
        thread: u32,
        process: u32,
        base_priority: i32,
        delta_priority: i32,
        flags: u32,
    }

    #[repr(C, align(16))]
    struct Context([u8; 1232]);

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, process: u32) -> Handle;
        fn Thread32First(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
        fn Thread32Next(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
        fn OpenThread(access: u32, inherit: i32, thread: u32) -> Handle;
        fn SuspendThread(thread: Handle) -> u32;
        fn ResumeThread(thread: Handle) -> u32;
        fn GetThreadContext(thread: Handle, context: *mut Context) -> i32;
        fn GetCurrentProcessId() -> u32;
        fn GetCurrentThreadId() -> u32;
        fn GetCurrentProcess() -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn ReadProcessMemory(process: Handle, address: *const c_void, buffer: *mut c_void, size: usize, read: *mut usize) -> i32;
        fn Sleep(milliseconds: u32);
        fn RtlLookupFunctionEntry(pc: u64, base: *mut u64, history: *mut c_void) -> *mut c_void;
        fn RtlVirtualUnwind(kind: u32, base: u64, pc: u64, entry: *mut c_void, context: *mut Context,
            handler: *mut *mut c_void, frame: *mut u64, pointers: *mut c_void) -> *mut c_void;
    }

    #[link(name = "winmm")]
    extern "system" {
        fn timeBeginPeriod(period: u32) -> u32;
    }

    #[repr(C)]
    struct SymbolInfo {
        size: u32,
        type_index: u32,
        reserved: [u64; 2],
        index: u32,
        symbol_size: u32,
        module_base: u64,
        flags: u32,
        value: u64,
        address: u64,
        register: u32,
        scope: u32,
        tag: u32,
        name_length: u32,
        max_name_length: u32,
        name: [u16; 512],
    }

    #[link(name = "dbghelp")]
    extern "system" {
        fn SymSetOptions(options: u32) -> u32;
        fn SymInitializeW(process: Handle, search: *const u16, invade: i32) -> i32;
        fn SymFromAddrW(process: Handle, address: u64, displacement: *mut u64, symbol: *mut SymbolInfo) -> i32;
    }

    pub struct Sampler {
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<Vec<[u64; DEPTH]>>>,
        report: std::path::PathBuf,
    }

    struct Threads {
        handles: [Handle; MAX_THREADS],
        ids: [u32; MAX_THREADS],
        count: usize,
    }

    impl Threads {
        /// Opens every other thread of this process not yet held.
        fn refresh(&mut self, own: u32) {
            // SAFETY: plain Win32 calls with owned, sized buffers.
            unsafe {
                let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
                if snapshot.is_null() || snapshot as isize == -1 { return }
                let process = GetCurrentProcessId();
                let mut entry = ThreadEntry { size: std::mem::size_of::<ThreadEntry>() as u32, usage: 0, thread: 0, process: 0, base_priority: 0, delta_priority: 0, flags: 0 };
                let mut more = Thread32First(snapshot, &mut entry) != 0;
                while more {
                    if entry.process == process && entry.thread != own && self.count < MAX_THREADS && !self.ids[..self.count].contains(&entry.thread) {
                        let handle = OpenThread(THREAD_ACCESS, 0, entry.thread);
                        if !handle.is_null() {
                            self.handles[self.count] = handle;
                            self.ids[self.count] = entry.thread;
                            self.count += 1;
                        }
                    }
                    more = Thread32Next(snapshot, &mut entry) != 0;
                }
                CloseHandle(snapshot);
            }
        }
    }

    /// One thread's call stack (instruction pointers, innermost first), read
    /// while it is suspended by unwinding its context with the image's
    /// unwind tables. Allocates nothing: a suspended thread may hold a lock
    /// the allocator needs.
    fn sample(thread: Handle, context: &mut Context, into: &mut [u64; DEPTH]) -> bool {
        // SAFETY: the context buffer is CONTEXT-sized and 16-byte aligned;
        // unwinding reads the suspended thread's own stack in this process.
        unsafe {
            if SuspendThread(thread) == u32::MAX { return false }
            context.0[OFFSET_FLAGS..OFFSET_FLAGS + 4].copy_from_slice(&CONTEXT_FLAGS.to_le_bytes());
            let ok = GetThreadContext(thread, context) != 0;
            if ok {
                into.fill(0);
                let word = |context: &Context, offset: usize| u64::from_le_bytes(context.0[offset..offset + 8].try_into().unwrap_or_default());
                let process = GetCurrentProcess();
                for slot in into.iter_mut() {
                    let pc = word(context, OFFSET_RIP);
                    if pc == 0 { break }
                    *slot = pc;
                    let mut base = 0u64;
                    let entry = RtlLookupFunctionEntry(pc, &mut base, std::ptr::null_mut());
                    if entry.is_null() {
                        // A leaf function: its return address is on top.
                        let rsp = word(context, OFFSET_RSP);
                        let mut ret = 0u64;
                        let mut got = 0usize;
                        if ReadProcessMemory(process, rsp as *const c_void, (&mut ret as *mut u64).cast(), 8, &mut got) == 0 || got != 8 { break }
                        context.0[OFFSET_RIP..OFFSET_RIP + 8].copy_from_slice(&ret.to_le_bytes());
                        context.0[OFFSET_RSP..OFFSET_RSP + 8].copy_from_slice(&(rsp + 8).to_le_bytes());
                    } else {
                        let mut handler = std::ptr::null_mut();
                        let mut frame = 0u64;
                        RtlVirtualUnwind(0, base, pc, entry, context, &mut handler, &mut frame, std::ptr::null_mut());
                    }
                }
            }
            ResumeThread(thread);
            ok
        }
    }

    pub fn start(report: std::path::PathBuf) -> Sampler {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::spawn(move || {
            // SAFETY: plain Win32 calls.
            let own = unsafe {
                timeBeginPeriod(1);
                GetCurrentThreadId()
            };
            let mut threads = Threads { handles: [std::ptr::null_mut(); MAX_THREADS], ids: [0; MAX_THREADS], count: 0 };
            let mut samples: Vec<[u64; DEPTH]> = Vec::with_capacity(CAPACITY);
            let mut context = Box::new(Context([0; 1232]));
            let mut tick = 0u64;
            while !stopping.load(Ordering::Relaxed) && samples.len() + MAX_THREADS < CAPACITY {
                if tick % 50 == 0 { threads.refresh(own) }
                tick += 1;
                for &handle in &threads.handles[..threads.count] {
                    let mut frames = [0u64; DEPTH];
                    if sample(handle, &mut context, &mut frames) && frames[0] != 0 { samples.push(frames) }
                }
                // SAFETY: plain Win32 call.
                unsafe { Sleep(1) }
            }
            for &handle in &threads.handles[..threads.count] {
                // SAFETY: handles this thread opened.
                unsafe { CloseHandle(handle); }
            }
            samples
        });
        Sampler { stop, thread: Some(thread), report }
    }

    impl Drop for Sampler {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            let Some(samples) = self.thread.take().and_then(|thread| thread.join().ok()) else { return };
            let _ = std::fs::write(&self.report, report(&samples));
        }
    }

    fn report(samples: &[[u64; DEPTH]]) -> String {
        // SAFETY: dbghelp on this process, from one thread.
        let process = unsafe { GetCurrentProcess() };
        // The PDB sits beside the executable.
        let search: Vec<u16> = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.as_os_str().to_owned()))
            .map(|dir| std::os::windows::ffi::OsStrExt::encode_wide(dir.as_os_str()).chain([0]).collect()).unwrap_or_else(|| vec![0]);
        unsafe {
            SymSetOptions(SYMBOL_OPTIONS);
            SymInitializeW(process, search.as_ptr(), 1);
        }
        let mut names: HashMap<u64, std::rc::Rc<str>> = HashMap::new();
        let mut name = |address: u64| -> std::rc::Rc<str> {
            names.entry(address).or_insert_with(|| {
                let mut symbol = Box::new(SymbolInfo {
                    size: 88, type_index: 0, reserved: [0; 2], index: 0, symbol_size: 0, module_base: 0, flags: 0,
                    value: 0, address: 0, register: 0, scope: 0, tag: 0, name_length: 0, max_name_length: 511, name: [0; 512],
                });
                let mut displacement = 0u64;
                // SAFETY: a SYMBOL_INFOW with room for 511 name characters.
                if unsafe { SymFromAddrW(process, address, &mut displacement, &mut *symbol) } == 0 { return format!("?{address:x}").into() }
                let length = (symbol.name_length as usize).min(511);
                String::from_utf16_lossy(&symbol.name[..length]).into()
            }).clone()
        };
        let mut own: HashMap<std::rc::Rc<str>, usize> = HashMap::new();
        let mut inclusive: HashMap<std::rc::Rc<str>, usize> = HashMap::new();
        let mut seen = Vec::new();
        let mut busy = 0usize;
        for frames in samples {
            let leaf = name(frames[0]);
            // Threads waiting for work are not CPU time.
            if leaf.contains("Wait") { continue }
            busy += 1;
            *own.entry(leaf).or_default() += 1;
            seen.clear();
            for (depth, &address) in frames.iter().enumerate() {
                if address == 0 { break }
                // A return address points after its call.
                let function = name(if depth == 0 { address } else { address - 1 });
                if !seen.contains(&function) { seen.push(function) }
            }
            for function in &seen { *inclusive.entry(function.clone()).or_default() += 1 }
        }
        // Callers of each function named in ZLINT_SAMPLE_CALLERS (`;`
        // separated name parts, `>part` for its callees), by name.
        let focus: Vec<String> = std::env::var("ZLINT_SAMPLE_CALLERS").map(|list| list.split(';').map(str::to_owned).collect()).unwrap_or_default();
        let mut callers: Vec<HashMap<std::rc::Rc<str>, usize>> = focus.iter().map(|_| HashMap::new()).collect();
        if !focus.is_empty() {
            for frames in samples {
                let depth = frames.iter().take_while(|&&address| address != 0).count();
                let names: Vec<_> = (0..depth).map(|at| name(if at == 0 { frames[0] } else { frames[at] - 1 })).collect();
                if names.first().is_some_and(|leaf| leaf.contains("Wait")) { continue }
                for (index, part) in focus.iter().enumerate() {
                    // `>name`: what it calls (outermost occurrence) instead.
                    let (callees, part) = match part.strip_prefix('>') { Some(part) => (true, part), None => (false, part.as_str()) };
                    let mut counted: Vec<std::rc::Rc<str>> = Vec::new();
                    if callees {
                        let Some(outer) = names.iter().rposition(|name| name.contains(part)) else { continue };
                        let callee = if outer == 0 { std::rc::Rc::from("(self)") } else { names[outer - 1].clone() };
                        *callers[index].entry(callee).or_default() += 1;
                        continue;
                    }
                    for at in 0..names.len() {
                        if !names[at].contains(part) { continue }
                        let Some(caller) = names[at + 1..].iter().find(|caller| !caller.contains(part)) else { continue };
                        if !counted.contains(caller) {
                            counted.push(caller.clone());
                            *callers[index].entry(caller.clone()).or_default() += 1;
                        }
                    }
                }
            }
        }
        let total = busy.max(1);
        let table = |counts: &HashMap<std::rc::Rc<str>, usize>, title: &str| {
            let mut rows: Vec<_> = counts.iter().collect();
            rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
            let mut out = format!("{title}\n");
            for (function, count) in rows.into_iter().take(150) {
                out.push_str(&format!("{count:>8} {:>6.2}%  {function}\n", *count as f64 * 100.0 / total as f64));
            }
            out
        };
        let mut out = format!("samples {} (busy {busy})\n\n{}\n{}", samples.len(), table(&own, "self"), table(&inclusive, "inclusive"));
        for (part, counts) in focus.iter().zip(&callers) {
            out.push('\n');
            out.push_str(&table(counts, &format!("callers of {part} (`>`: callees)")));
        }
        out
    }
}

/// A running sampler that reports when dropped; `None` unless
/// `ZLINT_SAMPLE` names a report path.
#[cfg(windows)]
pub fn start_from_env() -> Option<impl Drop> {
    std::env::var_os("ZLINT_SAMPLE").map(|path| windows::start(path.into()))
}

#[cfg(not(windows))]
pub fn start_from_env() -> Option<()> {
    None
}
