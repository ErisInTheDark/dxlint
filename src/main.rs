#[global_allocator]
static ALLOCATOR: zoomie_lint::alloc::Pooled = zoomie_lint::alloc::Pooled;

fn main() -> std::process::ExitCode {
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
    std::process::ExitCode::from(code)
}
