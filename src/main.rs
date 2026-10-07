//! pnet_installer — bootstrap pNet onto this machine.
//!
//! Copies a local or downloaded `pnet` binary, writes first-run parameters,
//! and starts the node. This program does not register as a pNet app.

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
            let outcome = (|| {
                bootstrap::prepare_setup(&mut opts)?;
                let plan = bootstrap::plan(&opts)?;
                bootstrap::execute(&opts, &plan)
            })();
            if let Some(staged) = opts.staged.as_ref() {
                let _ = std::fs::remove_dir_all(&staged.dir);
            }
            match outcome {
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
