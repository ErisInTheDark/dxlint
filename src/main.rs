#[global_allocator]
static ALLOCATOR: dxlint::alloc::Pooled = dxlint::alloc::Pooled;

fn main() -> std::process::ExitCode {
    dxlint::no_error_dialogs();
    let code = {
        // The sampler reports when it drops, before the levers do.
        let _sampler = dxlint::sampler::start_from_env();
        // Everything runs on a worker-sized stack: a file linted on this thread
        // (fixtures, the language server's loop) asks questions as deep as a
        // lint worker's, and the process main thread's stack is far smaller.
        let run = std::thread::Builder::new().stack_size(dxlint::engine::WORKER_STACK).spawn(|| {
            let cwd = std::env::current_dir().unwrap_or_default();
            dxlint::cli::run(
                std::env::args_os(),
                &cwd,
                std::io::stdin(),
                anstream::AutoStream::new(std::io::stdout(), anstream::ColorChoice::Auto),
                anstream::AutoStream::new(std::io::stderr(), anstream::ColorChoice::Auto),
            )
        }).expect("main thread");
        run.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    };
    dxlint::levers::report();
    std::process::ExitCode::from(code)
}
