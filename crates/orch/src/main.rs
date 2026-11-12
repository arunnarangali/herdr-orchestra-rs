//! `orch` binary: CLI parsing and command dispatch only.

mod commands;

use clap::{Parser, Subcommand};
use commands::{COMMAND_NAMES, EXIT_NOT_IMPLEMENTED, usage};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(
    name = "orch",
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true
)]
struct Cli {
    /// Print the version and exit (`-v` / `--version`).
    #[arg(short = 'v', long = "version")]
    version: bool,
    /// Print usage and exit.
    #[arg(short = 'h', long = "help")]
    help: bool,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print the version.
    Version,
    /// Any other command: name first, then its arguments (validated against the command table).
    #[command(external_subcommand)]
    External(Vec<String>),
}

fn main() {
    std::process::exit(run(std::env::args_os()));
}

fn run<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            print!("{}", usage());
            return 2;
        }
    };
    if cli.version {
        return print_version();
    }
    if cli.help {
        print!("{}", usage());
        return 0;
    }
    match cli.command {
        None => {
            print!("{}", usage());
            0
        }
        Some(Cmd::Version) => print_version(),
        Some(Cmd::External(args)) => {
            let name = args.first().map(String::as_str).unwrap_or_default();
            if !COMMAND_NAMES.contains(&name) {
                println!("{}", usage());
                return 2;
            }
            orch_core::logging::init();
            tracing::debug!(command = name, "dispatch");
            if let Some(code) = commands::dispatch(&args) {
                return code;
            }
            eprintln!("orch {name}: not implemented in this build (exit {EXIT_NOT_IMPLEMENTED})");
            EXIT_NOT_IMPLEMENTED
        }
    }
}

fn print_version() -> i32 {
    println!("herdr-orchestra {VERSION}");
    0
}
