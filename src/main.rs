#[global_allocator]
static ALLOCATOR: zoomie_lint::alloc::Pooled = zoomie_lint::alloc::Pooled;

fn main() -> std::process::ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    std::process::ExitCode::from(zoomie_lint::cli::run(
        std::env::args_os(),
        &cwd,
        std::io::stdin(),
        anstream::AutoStream::new(std::io::stdout(), anstream::ColorChoice::Auto),
        anstream::AutoStream::new(std::io::stderr(), anstream::ColorChoice::Auto),
    ))
}
