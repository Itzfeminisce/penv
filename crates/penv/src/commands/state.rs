use std::path::Path;

use serde_json::json;

use crate::error::CliError;
use crate::files::{ENV_FILE, SCHEMA_FILE, find_schema, read_file, show};
use crate::output::{Output, Report};

/// The state, and the one command that follows from it.
pub fn run(out: &Output, cwd: &Path) -> Result<Report, CliError> {
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

    let style = out.style();
    let mut text = format!(
        "{}   {}\n{}    {}",
        style.dim("state"),
        style.bold(state),
        style.dim("next"),
        next
    );
    text.push('\n');
    text.push_str(&style.dim(&note));

    Ok(Report::new(
        json!({
            "state": state,
            "schema": schema_path.as_deref().map(show),
            "project": project,
            "next": next,
            "note": note,
        }),
        text,
    ))
}
