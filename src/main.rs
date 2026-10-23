#[global_allocator]
static ALLOCATOR: zoomie_lint::alloc::Pooled = zoomie_lint::alloc::Pooled;

/// A crash ends the process with its exit code instead of a modal error
/// dialog: zlint runs unattended (CI, editors, batch runs), where a dialog
/// blocks until someone clicks it. Child processes inherit the mode.
#[cfg(windows)]
fn no_error_dialogs() {
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;
    const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
    extern "system" {
        fn SetErrorMode(mode: u32) -> u32;
    }
    // SAFETY: SetErrorMode only sets this process's error mode.
    unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX); }
}

#[cfg(not(windows))]
fn no_error_dialogs() {}

fn main() -> std::process::ExitCode {
    no_error_dialogs();
    let sampler = zoomie_lint::sampler::start_from_env();
    // Everything runs on a worker-sized stack: a file linted on this thread
    // (fixtures, the language server's loop) asks questions as deep as a
    // lint worker's, and the process main thread's stack is far smaller.
    let run = std::thread::Builder::new().stack_size(zoomie_lint::engine::WORKER_STACK).spawn(|| {
        let cwd = std::env::current_dir().unwrap_or_default();
        zoomie_lint::cli::run(
            std::env::args_os(),
            &cwd,
            std::io::stdin(),
            anstream::AutoStream::new(std::io::stdout(), anstream::ColorChoice::Auto),
            anstream::AutoStream::new(std::io::stderr(), anstream::ColorChoice::Auto),
        )
    }).expect("main thread");
    let code = run.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    drop(sampler);
    std::process::ExitCode::from(code)
}
