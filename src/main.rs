//! pnet_installer — bootstrap pNet onto this machine.
//!
//! Copies a local `pnet` binary, writes first-run parameters, and starts the
//! node. This program does not register as a pNet app.

use std::path::PathBuf;

use pnet_installer::bootstrap::{self, Cmd};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match bootstrap::parse_args(&args) {
        Ok(Cmd::Help) => {
            print!("{}", bootstrap::help_text());
        }
        Ok(Cmd::Bootstrap(mut opts)) => {
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("pnet_installer"));
            if let Err(e) = bootstrap::resolve_from(&mut opts, &exe) {
                eprintln!("[bootstrap] {e}");
                std::process::exit(1);
            }
            if let Err(e) = bootstrap::prepare_setup(&mut opts) {
                eprintln!("[bootstrap] {e}");
                std::process::exit(1);
            }
            match bootstrap::plan(&opts).and_then(|p| bootstrap::execute(&opts, &p)) {
                Ok(log) => print!("{log}"),
                Err(e) => {
                    eprintln!("[bootstrap] {e}");
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("[bootstrap] {e}\n");
            print!("{}", bootstrap::help_text());
            std::process::exit(1);
        }
    }
}
