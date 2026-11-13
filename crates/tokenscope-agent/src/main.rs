//! M0-12 Agent CLI entry point and lifecycle modules.
mod antigravity_rpc;
mod background_quota;
mod cli;
mod codex_rpc;
mod collection;
mod daemon;
mod diagnostics;
mod identity;
mod lock;
mod pi;
mod pricing;
mod quota;
mod session_control;
mod shutdown;
mod sync;

fn main() {
    if let Err(err) = cli::run() {
        eprintln!("TokenScope: {err}");
        std::process::exit(1);
    }
}
