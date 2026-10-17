mod cli;
mod dates;
mod entrypoints;
mod git;
mod init;
mod model;
mod parse;
mod render;
mod repository;
mod scan;

use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let response = cli::run(&args);
    if let Err(error) = io::stdout().lock().write_all(response.stdout.as_bytes()) {
        if error.kind() == io::ErrorKind::BrokenPipe {
            return ExitCode::SUCCESS;
        }
        eprintln!("Could not write output: {error}");
        return ExitCode::from(2);
    }
    #[expect(
        clippy::let_underscore_must_use,
        reason = "stderr unwritable: nothing left to report to; the exit code still signals failure"
    )]
    let _ = io::stderr().lock().write_all(response.stderr.as_bytes());
    ExitCode::from(response.code)
}
