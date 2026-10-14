//! The `janky` binary. It parses the command line and runs it with [`janky_cli::cli::run`].

use std::process::ExitCode;

use clap::Parser;
use janky_cli::Cli;

fn main() -> ExitCode {
    janky_cli::cli::run(&Cli::parse())
}
