//! Generate native configuration files from a single Pkl source.
#![warn(unreachable_pub)]

mod cli;
mod compiler;
mod config;
mod ignore;
mod publication;
mod watch;

fn main() -> std::process::ExitCode {
    match cli::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("confset: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
