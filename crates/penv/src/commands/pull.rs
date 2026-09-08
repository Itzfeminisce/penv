use std::io::IsTerminal;
use std::path::Path;

use penv_agent::Policy;
use penv_cloud::api::Fetched;
use penv_dotenv::ensure_ignored;
use serde_json::json;

use crate::agent::detect_here;
use crate::commands::cloud::{Cloud, address, environment, refuse};
use crate::env::Env;
use crate::error::{CliError, Exit};
use crate::files::{ENV_FILE, GITIGNORE_FILE, read_file, show, write_file, write_private_file};
use crate::output::{Output, Report};

/// Write a plain `.env` from the cloud. Only a person asks for this.
pub fn run(
    out: &Output,
    cwd: &Path,
    env_flag: Option<&str>,
    i_am_human: bool,
    env: &Env,
    agent_flag: bool,
) -> Result<Report, CliError> {
    let detection = detect_here(env, std::io::stdout().is_terminal());
    let policy = Policy::for_(&detection, agent_flag);
    if !policy.pull_allowed && !i_am_human {
        return Err(CliError::new(
            "agent_session",
            format!(
                "penv pull writes every value to disk, and this session is {}.",
                detection.name().unwrap_or("an agent")
            ),
            "Run it yourself with --i-am-human, or let penv run inject the values instead.",
        )
        .with_exit(Exit::Auth));
    }

    let (schema_path, schema) = super::load_schema(cwd)?;
    let dir = schema_path.parent().unwrap_or(cwd).to_path_buf();
    let at = address(&schema, &environment(env_flag, env))?;

    let cloud = Cloud::open(env, &detection)?;
    let bearer = cloud.bearer(env, schema.org.as_deref())?;
    let Fetched::Body { body, .. } = cloud
        .api
        .env_get(&bearer, &at, None, true)
        .map_err(|e| refuse(e, Some(&at)))?
    else {
        return Err(CliError::new(
            "unexpected_answer",
            "the server said nothing had changed for a read that asked for everything.",
            "Try again.",
        ));
    };

    let pairs: Vec<(&str, &str)> = body
        .keys
        .iter()
        .filter_map(|key| Some((key.name.as_str(), key.value.as_deref()?)))
        .collect();
    let contents = penv_dotenv::write(&pairs).map_err(|e| {
        CliError::new(
            "unwritable_value",
            e.to_string(),
            "Fix the value in the console, then pull again.",
        )
    })?;

    let env_path = dir.join(ENV_FILE);
    write_private_file(&env_path, &contents)?;

    let ignore_path = dir.join(GITIGNORE_FILE);
    let existing = if ignore_path.is_file() {
        read_file(&ignore_path)?
    } else {
        String::new()
    };
    let update = ensure_ignored(&existing);
    if update.changed() {
        write_file(&ignore_path, &update.content)?;
    }

    let style = out.style();
    let mut lines = vec![format!(
        "{} {} key(s) into {}",
        style.green("wrote"),
        pairs.len(),
        show(&env_path)
    )];
    if !body.skipped.is_empty() {
        lines.push(style.dim(&format!(
            "{} key(s) the cloud resolves itself were skipped",
            body.skipped.len()
        )));
    }

    Ok(Report::new(
        json!({
            "address": at.to_string(),
            "env": show(&env_path),
            "keys": pairs.len(),
            "skipped": body.skipped,
            "gitignore": { "path": show(&ignore_path), "added": update.added },
        }),
        lines.join("\n"),
    ))
}
