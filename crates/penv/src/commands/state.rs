use std::io::IsTerminal;
use std::path::Path;

use penv_agent::{Detection, Policy};
use serde_json::{Value, json};

use crate::agent::detect_here;
use crate::commands::cloud::{self, Cloud};
use crate::env::Env;
use crate::error::CliError;
use crate::files::{ENV_FILE, SCHEMA_FILE, find_schema, read_file, show};
use crate::output::{Output, Report};

/// The state, and the one command that follows from it.
pub fn run(out: &Output, cwd: &Path, env: &Env) -> Result<Report, CliError> {
    let schema_path = find_schema(cwd);
    let has_env = cwd.join(ENV_FILE).is_file();
    let detection = detect_here(env, std::io::stdout().is_terminal());
    let policy = Policy::for_(&detection, false);

    let schema = match &schema_path {
        Some(path) => penv_schema::parse(&read_file(path)?).ok(),
        None => None,
    };
    let cloud = schema.as_ref().filter(|s| s.is_cloud()).map(|s| {
        (
            s.org.clone().unwrap_or_default(),
            s.project.clone().unwrap_or_default(),
        )
    });

    let (state, next, note, credential, cache_age) = match (&schema_path, &cloud) {
        (None, _) if has_env => (
            "unconfigured",
            "penv init",
            format!("There is a {ENV_FILE} here and no schema for it yet."),
            None,
            None,
        ),
        (None, _) => (
            "unconfigured",
            "penv init",
            format!("Write a {ENV_FILE} first; penv init reads it."),
            None,
            None,
        ),
        (Some(_), None) => (
            "local",
            "penv check",
            format!("Values come from {ENV_FILE} next to the schema."),
            None,
            None,
        ),
        (Some(_), Some((org, project))) => {
            let (held, age) = cloud_state(env, &detection, org, project);
            let (next, note) = if held {
                (
                    "penv run",
                    format!("{SCHEMA_FILE} names {org}/{project}, and this host has a credential."),
                )
            } else {
                (
                    "penv login",
                    format!(
                        "{SCHEMA_FILE} names {org}/{project}, and this host has no credential."
                    ),
                )
            };
            ("cloud", next, note, Some(held), age)
        }
    };

    let style = out.style();
    let mut text = format!(
        "{}   {}\n{}    {}",
        style.dim("state"),
        style.bold(state),
        style.dim("next"),
        next
    );
    if let Some((org, project)) = &cloud {
        text.push_str(&format!(
            "\n{} {org}/{project}\n{} {}",
            style.dim("project"),
            style.dim("credential"),
            if credential == Some(true) {
                "present"
            } else {
                "none"
            }
        ));
        text.push_str(&format!(
            "\n{}   {}",
            style.dim("cache"),
            match cache_age {
                Some(seconds) => format!("{seconds}s old"),
                None => "empty".to_string(),
            }
        ));
    }
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
            "project": cloud.as_ref().map(|(org, project)| format!("{org}/{project}")),
            "org": cloud.as_ref().map(|(org, _)| org.clone()),
            "credential": credential,
            "cacheAge": cache_age,
            "next": next,
            "note": note,
            "agent": agent_json(&detection),
            "masking": policy.mask,
        }),
        text,
    ))
}

/// Whether this host can prove itself, and how old its cached development
/// values are. Which credential it holds is never said.
fn cloud_state(env: &Env, detection: &Detection, org: &str, project: &str) -> (bool, Option<u64>) {
    let Ok(opened) = Cloud::open(env, detection) else {
        return (false, None);
    };
    let held = penv_cloud::credential::present(env.as_map(), opened.keychain.as_ref());
    let at = penv_cloud::Address::new(org, project, cloud::DEFAULT_ENVIRONMENT);
    let age = opened.cache(&at).and_then(|cache| cache.age(opened.now));
    (held, age)
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
