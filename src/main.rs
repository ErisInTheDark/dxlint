#[global_allocator]
static ALLOCATOR: dxlint::alloc::Pooled = dxlint::alloc::Pooled;

fn main() -> std::process::ExitCode {
    dxlint::no_error_dialogs();
    std::process::ExitCode::from(dxlint::cli::run_process(std::env::args_os().collect(), anstream::ColorChoice::Auto))
}
