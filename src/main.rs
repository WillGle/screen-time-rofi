use std::env;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn usage() {
    eprintln!(
        "screen-time {VERSION}

Usage:
  screen-time daemon              run the per-user tracker
  screen-time render --format rofi
                                  render the main dashboard / app detail
  screen-time doctor              check runtime, DB, and snapshot health
  screen-time --version           print version

M0 scaffold: subcommands are placeholders."
    );
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("daemon") => {
            eprintln!("screen-time: daemon is not implemented yet (M0 scaffold)");
            ExitCode::from(2)
        }
        Some("render") => {
            eprintln!("screen-time: render is not implemented yet (M0 scaffold)");
            ExitCode::from(2)
        }
        Some("doctor") => {
            eprintln!("screen-time: doctor is not implemented yet (M0 scaffold)");
            ExitCode::from(2)
        }
        Some("--version" | "-V") => {
            println!("screen-time {VERSION}");
            ExitCode::SUCCESS
        }
        Some("-h" | "--help") | None => {
            usage();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("screen-time: unknown command: {other}");
            usage();
            ExitCode::from(2)
        }
    }
}
