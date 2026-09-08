mod check;
pub mod cloud;
mod r#gen;
pub mod guard;
pub mod hook;
mod init;
mod login;
mod logout;
mod ls;
mod machine;
mod pull;
mod push;
mod reveal;
mod run;
mod set;
mod state;

use std::path::Path;

use crate::cli::{Cli, Command, MachineCommand};
use crate::env::Env;
use crate::error::CliError;
use crate::files::{SCHEMA_FILE, find_schema, read_file};
use crate::manifest::manifest;
use crate::output::{Output, Report};
use penv_schema::Schema;

pub fn dispatch(cli: &Cli, out: &Output, cwd: &Path, env: &Env) -> Result<Report, CliError> {
    match &cli.command {
        None => state::run(out, cwd, env),
        Some(Command::Init { force }) => init::run(out, cwd, *force),
        Some(Command::Run {
            env: environment,
            no_mask,
            command,
        }) => run::run(
            out,
            cwd,
            environment.as_deref(),
            *no_mask,
            command,
            env,
            cli.agent,
        ),
        Some(Command::Push {
            env: environment,
            org,
            prune,
        }) => push::run(
            out,
            cwd,
            environment.as_deref(),
            org.as_deref(),
            *prune,
            env,
        ),
        Some(Command::Pull {
            env: environment,
            i_am_human,
        }) => pull::run(
            out,
            cwd,
            environment.as_deref(),
            *i_am_human,
            env,
            cli.agent,
        ),
        Some(Command::Login) => login::run(out, cwd, env, cli.agent),
        Some(Command::Logout) => logout::run(out, cwd, env),
        Some(Command::Set {
            key,
            env: environment,
            value,
        }) => set::set(out, cwd, key, environment.as_deref(), value.as_deref(), env),
        Some(Command::Unset {
            key,
            env: environment,
        }) => set::unset(out, cwd, key, environment.as_deref(), env),
        Some(Command::Reveal {
            key,
            env: environment,
        }) => reveal::run(out, cwd, key, environment.as_deref(), env, cli.agent),
        Some(Command::Machine {
            command: MachineCommand::Enroll { secret },
        }) => machine::enroll(out, cwd, secret, env),
        Some(Command::Check { key }) => check::run(out, cwd, key.as_deref()),
        Some(Command::Ls) => ls::run(out, cwd),
        Some(Command::Gen {
            target,
            out: to,
            check,
        }) => r#gen::run(out, cwd, target.as_deref(), to.as_deref(), *check),
        Some(Command::Guard {
            harness,
            all,
            check,
        }) => guard::run(out, cwd, *check, *all, harness),
        Some(Command::Hook { harness }) => hook::run(harness, cwd),
        Some(Command::Schema) => schema(cwd),
        Some(Command::Help { command }) => help(command.as_deref()),
        Some(other) => Err(CliError::not_in_this_build(&path_of(other))),
    }
}

fn path_of(command: &Command) -> String {
    match command {
        Command::Run { .. } => "run".into(),
        Command::Push { .. } => "push".into(),
        Command::Pull { .. } => "pull".into(),
        Command::Login => "login".into(),
        Command::Logout => "logout".into(),
        Command::Set { .. } => "set".into(),
        Command::Unset { .. } => "unset".into(),
        Command::Gen { .. } => "gen".into(),
        Command::Guard { .. } => "guard".into(),
        Command::Reveal { .. } => "reveal".into(),
        Command::Machine { command } => match command {
            MachineCommand::Enroll { .. } => "machine enroll".into(),
        },
        Command::Upgrade => "upgrade".into(),
        Command::Completions { .. } => "completions".into(),
        Command::Hook { .. } => "hook".into(),
        Command::Init { .. } => "init".into(),
        Command::Check { .. } => "check".into(),
        Command::Ls => "ls".into(),
        Command::Schema => "schema".into(),
        Command::Help { .. } => "help".into(),
    }
}

/// Load the nearest schema, reporting its diagnostics as one validation failure.
fn load_schema(cwd: &Path) -> Result<(std::path::PathBuf, Schema), CliError> {
    let Some(path) = find_schema(cwd) else {
        return Err(CliError::new(
            "no_schema",
            format!("no {SCHEMA_FILE} here or in any directory above."),
            "Run penv init to write one from your .env.",
        ));
    };
    let source = read_file(&path)?;
    penv_schema::parse(&source)
        .map(|schema| (path.clone(), schema))
        .map_err(|diagnostics| {
            let listed = diagnostics
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            CliError::new(
                "invalid_schema",
                format!(
                    "{} has {} problem(s): {listed}",
                    crate::files::show(&path),
                    diagnostics.len()
                ),
                "Run penv check for the list, fix the lines it names, then try again.",
            )
            .with_exit(crate::error::Exit::Validation)
        })
}

fn schema(cwd: &Path) -> Result<Report, CliError> {
    let (_, schema) = load_schema(cwd)?;
    let json = schema.to_json();
    let text = serde_json::to_string_pretty(&json).unwrap_or_default();
    Ok(Report::new(json, text))
}

/// `help --json` is the manifest; on a terminal it is clap's own help text.
fn help(command: Option<&str>) -> Result<Report, CliError> {
    use clap::CommandFactory;

    let mut root = Cli::command();
    let text = match command {
        None => root.render_long_help().to_string(),
        Some(name) => root
            .get_subcommands()
            .find(|c| c.get_name() == name)
            .cloned()
            .ok_or_else(|| {
                CliError::new(
                    "unknown_command",
                    format!("penv has no {name} command."),
                    "Run penv help to see the tree.",
                )
            })?
            .render_long_help()
            .to_string(),
    };
    Ok(Report::new(manifest(), text))
}
