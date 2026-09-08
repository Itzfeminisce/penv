use std::path::Path;

use penv_dotenv::{ensure_ignored, infer, read};
use penv_schema::render;
use serde_json::json;

use crate::error::{CliError, Exit};
use crate::files::{ENV_FILE, GITIGNORE_FILE, SCHEMA_FILE, read_file, show, write_file};
use crate::output::{Output, Report, table};

/// Read the `.env` here, write the schema it implies, and ignore the file.
pub fn run(out: &Output, cwd: &Path, force: bool) -> Result<Report, CliError> {
    let env_path = cwd.join(ENV_FILE);
    if !env_path.is_file() {
        return Err(CliError::new(
            "no_dotenv",
            format!("there is no {ENV_FILE} in this directory."),
            format!("Write a {ENV_FILE} with your keys, then run penv init."),
        ));
    }

    let schema_path = cwd.join(SCHEMA_FILE);
    if schema_path.is_file() && !force {
        return Err(CliError::new(
            "schema_exists",
            format!("{} already exists.", show(&schema_path)),
            "Run penv check to validate it, or penv init --force to write it again.",
        )
        .with_exit(Exit::Validation));
    }

    let env = read(&read_file(&env_path)?);
    let schema = infer(&env);
    write_file(&schema_path, &render(&schema))?;

    let ignore_path = cwd.join(GITIGNORE_FILE);
    let existing = if ignore_path.is_file() {
        read_file(&ignore_path)?
    } else {
        String::new()
    };
    let update = ensure_ignored(&existing);
    if update.changed() {
        write_file(&ignore_path, &update.content)?;
    }

    let rows: Vec<Vec<String>> = schema
        .keys
        .iter()
        .map(|key| {
            vec![
                key.name.clone(),
                key.ty.to_string(),
                yes_no(key.required),
                yes_no(key.sensitive),
            ]
        })
        .collect();

    let style = out.style();
    let mut lines = vec![
        table(&["KEY", "TYPE", "REQUIRED", "SENSITIVE"], &rows, &style),
        String::new(),
        style.dim(&format!(
            "wrote {} from {} keys",
            show(&schema_path),
            schema.keys.len()
        )),
    ];
    if update.changed() {
        lines.push(style.dim(&format!(
            "added {} to {}",
            update.added.join(", "),
            show(&ignore_path)
        )));
    }
    if !env.warnings.is_empty() {
        lines.push(style.dim(&format!(
            "{} line(s) in {ENV_FILE} sit outside the safe subset; penv check names them",
            env.warnings.len()
        )));
    }
    let text = lines.join("\n");

    Ok(Report::new(
        json!({
            "schema": show(&schema_path),
            "keys": schema.keys.iter().map(|key| json!({
                "name": key.name,
                "type": key.ty.to_string(),
                "required": key.required,
                "sensitive": key.sensitive,
            })).collect::<Vec<_>>(),
            "gitignore": {
                "path": show(&ignore_path),
                "added": update.added,
            },
            "warnings": env.warnings.iter().map(|w| json!({
                "line": w.line,
                "code": w.code,
                "message": w.message,
            })).collect::<Vec<_>>(),
        }),
        text,
    ))
}

pub fn yes_no(value: bool) -> String {
    if value { "yes" } else { "no" }.to_string()
}
