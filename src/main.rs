mod cli;
mod lsp;

fn main() -> std::process::ExitCode {
    cli::run()
}
