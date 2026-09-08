use std::io::IsTerminal;
use std::path::Path;

use penv_agent::Policy;
use penv_cloud::api::Fetched;
use serde_json::json;

use crate::agent::detect_here;
use crate::commands::cloud::{Cloud, address, environment, refuse};
use crate::env::Env;
use crate::error::{CliError, Exit};
use crate::output::{Output, Report};

/// Print one value and nothing else. An agent session never gets here.
pub fn run(
    _out: &Output,
    cwd: &Path,
    name: &str,
    env_flag: Option<&str>,
    env: &Env,
    agent_flag: bool,
) -> Result<Report, CliError> {
    let detection = detect_here(env, std::io::stdout().is_terminal());
    let policy = Policy::for_(&detection, agent_flag);
    if !policy.reveal_allowed {
        return Err(CliError::new(
            "agent_session",
            format!(
                "penv reveal prints a value, and this session is {}.",
                detection.name().unwrap_or("an agent")
            ),
            "Read it yourself, or let penv run inject it into the process instead.",
        )
        .with_exit(Exit::Auth));
    }

    let (_, schema) = super::load_schema(cwd)?;
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

    let value = body
        .keys
        .iter()
        .find(|key| key.name == name)
        .and_then(|key| key.value.clone())
        .ok_or_else(|| {
            CliError::new(
                "no_value",
                format!("{name} has no value in {at}."),
                format!("Run penv ls to see the keys, or penv set {name} to give it one."),
            )
            .with_exit(Exit::Validation)
        })?;

    Ok(Report::new(json!({ "key": name, "value": value }), value))
}
