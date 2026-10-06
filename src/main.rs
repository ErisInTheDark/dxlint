#[global_allocator]
static ALLOCATOR: zoomie_lint::alloc::Pooled = zoomie_lint::alloc::Pooled;

mod cli;
mod lsp;

fn main() -> std::process::ExitCode {
    cli::run()
}
