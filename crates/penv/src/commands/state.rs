use std::io::IsTerminal;
use std::path::Path;

use penv_agent::{Detection, Policy};
use serde_json::{Value, json};

use crate::agent::detect_here;
use crate::env::Env;
use crate::error::CliError;
use crate::files::{ENV_FILE, SCHEMA_FILE, find_schema, read_file, show};
use crate::output::{Output, Report};

/// The state, and the one command that follows from it.
pub fn run(out: &Output, cwd: &Path, env: &Env) -> Result<Report, CliError> {
    let schema_path = find_schema(cwd);
    let has_env = cwd.join(ENV_FILE).is_file();

    let (state, project, next, note) = match &schema_path {
        None if has_env => (
            "unconfigured",
            None,
            "penv init",
            format!("There is a {ENV_FILE} here and no schema for it yet."),
        ),
        None => (
            "unconfigured",
            None,
            "penv init",
            format!("Write a {ENV_FILE} first; penv init reads it."),
        ),
        Some(path) => {
            let source = read_file(path)?;
            let project = penv_schema::parse(&source)
                .ok()
                .filter(|s| s.is_cloud())
                .map(|s| {
                    format!(
                        "{}/{}",
                        s.org.unwrap_or_default(),
                        s.project.unwrap_or_default()
                    )
                });
            match &project {
                Some(name) => (
                    "cloud",
                    project.clone(),
                    "penv login",
                    format!("{SCHEMA_FILE} names {name}."),
                ),
                None => (
                    "local",
                    None,
                    "penv check",
                    format!("Values come from {ENV_FILE} next to the schema."),
                ),
            }
        }
    };

    let detection = detect_here(env, std::io::stdout().is_terminal());
    let policy = Policy::for_(&detection, false);

    let style = out.style();
    let mut text = format!(
        "{}   {}\n{}    {}",
        style.dim("state"),
        style.bold(state),
        style.dim("next"),
        next
    );
    if let Some(name) = detection.name() {
        text.push_str(&format!(
            "\n{}   {} ({})\n{} {}",
            style.dim("agent"),
            style.bold(name),
            detection.confidence.as_str(),
            style.dim("masking"),
            if policy.mask { "on" } else { "off" },
        ));
    }
    text.push('\n');
    text.push_str(&style.dim(&note));

    Ok(Report::new(
        json!({
            "state": state,
            "schema": schema_path.as_deref().map(show),
            "project": project,
            "next": next,
            "note": note,
            "agent": agent_json(&detection),
            "masking": policy.mask,
        }),
        text,
    ))
}

fn agent_json(detection: &Detection) -> Value {
    match &detection.agent {
        None => Value::Null,
        Some(agent) => json!({
            "name": agent.name,
            "version": agent.version,
            "mode": agent.mode,
            "confidence": detection.confidence.as_str(),
            "session": detection.session_id,
            "markers": detection.markers,
        }),
    }
}
