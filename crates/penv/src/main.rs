use std::io::IsTerminal;

use clap::Parser;
use penv::cli::Cli;
use penv::commands;
use penv::env::Env;
use penv::error::Exit;
use penv::output::{Output, resolve};

fn main() {
    let cli = Cli::parse();
    let env = Env::from_process();
    let out = Output::new(resolve(
        cli.json,
        cli.agent,
        std::io::stdout().is_terminal(),
        &env,
    ));

    let cwd = std::env::current_dir().unwrap_or_default();
    let exit = match commands::dispatch(&cli, &out, &cwd) {
        Ok(report) => {
            let _ = out.write(&report, &mut std::io::stdout());
            report.exit
        }
        Err(error) => {
            let _ = out.fail(&error, &mut std::io::stderr());
            error.exit
        }
    };
    if exit != Exit::Ok {
        std::process::exit(exit as i32);
    }
}
