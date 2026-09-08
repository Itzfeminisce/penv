use std::path::Path;

use penv_schema::{Diagnostic, Values, Violation, extras, validate, validate_key};
use serde_json::{Value, json};

use crate::error::{CliError, Exit};
use crate::files::{ENV_FILE, SCHEMA_FILE, find_schema, read_file, show};
use crate::output::{Output, Report};

/// Schema problems and missing values, for every key or for one. Both are the
/// output, not an error, so they go to stdout and set exit 3.
pub fn run(out: &Output, cwd: &Path, only: Option<&str>) -> Result<Report, CliError> {
    let Some(schema_path) = find_schema(cwd) else {
        return Err(CliError::new(
            "no_schema",
            format!("no {SCHEMA_FILE} here or in any directory above."),
            "Run penv init to write one from your .env.",
        ));
    };
    let source = read_file(&schema_path)?;
    let style = out.style();

    let schema = match penv_schema::parse(&source) {
        Ok(schema) => schema,
        Err(diagnostics) => {
            let text = diagnostics
                .iter()
                .map(|d| {
                    format!(
                        "{} {}:{} {}",
                        style.red("fail"),
                        show(&schema_path),
                        d.line,
                        d.message
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(
                Report::new(body(&schema_path, None, &diagnostics, &[], &[], &[]), text)
                    .with_exit(Exit::Validation),
            );
        }
    };

    let dir = schema_path.parent().unwrap_or(cwd);
    let env_path = dir.join(ENV_FILE);
    let (values, warnings) = if env_path.is_file() {
        let env = penv_dotenv::read(&read_file(&env_path)?);
        (env.values(), env.warnings)
    } else {
        (Values::new(), Vec::new())
    };

    let violations = match only {
        None => validate(&schema, &values),
        Some(name) => {
            let key = schema.get(name).ok_or_else(|| {
                CliError::new(
                    "unknown_key",
                    format!("{name} is not in {}.", show(&schema_path)),
                    "Add a block for it, or run penv ls to see the keys.",
                )
                .with_exit(Exit::Validation)
            })?;
            validate_key(key, values.get(name).map(String::as_str))
        }
    };

    // Drift is a warning, not a violation: it never changes the exit code.
    let drift = match only {
        Some(_) => Vec::new(),
        None => extras(&schema, &values),
    };

    let mut lines: Vec<String> = Vec::new();
    if violations.is_empty() {
        let checked = only.map_or(schema.keys.len(), |_| 1);
        lines.push(format!(
            "{} {checked} key(s) in {}",
            style.green("ok"),
            show(&schema_path)
        ));
    }
    lines.extend(
        violations
            .iter()
            .map(|v| format!("{} {}", style.red("fail"), v.message)),
    );
    lines.extend(drift.iter().map(|key| {
        style.dim(&format!(
            "drift {key} is in {} and not in {}; penv masks it but never validates it",
            show(&env_path),
            show(&schema_path)
        ))
    }));
    lines.extend(warnings.iter().map(|w| {
        style.dim(&format!(
            "note {}:{} {}",
            show(&env_path),
            w.line,
            w.message
        ))
    }));

    let mut report = Report::new(
        body(
            &schema_path,
            env_path.is_file().then(|| show(&env_path)),
            &[],
            &violations,
            &warnings,
            &drift,
        ),
        lines.join("\n"),
    );
    if only.is_none() {
        // Guard coverage is part of a check; a stale guard is reported, never failed on.
        let guards = super::guard::run(out, cwd, true, false, &[])?;
        for harness in guards.json["harnesses"].as_array().into_iter().flatten() {
            for file in harness["files"].as_array().into_iter().flatten() {
                if file["status"] != "current" {
                    report.text.push_str(&format!(
                        "\n{} guard {} {} is {}; run penv guard",
                        style.dim("note"),
                        harness["name"].as_str().unwrap_or_default(),
                        file["path"].as_str().unwrap_or_default(),
                        file["status"].as_str().unwrap_or_default()
                    ));
                }
            }
        }
        report.json["guards"] = guards.json["harnesses"].clone();
    }
    Ok(if violations.is_empty() {
        report
    } else {
        report.with_exit(Exit::Validation)
    })
}

pub(super) fn body(
    schema_path: &Path,
    env_path: Option<String>,
    diagnostics: &[Diagnostic],
    violations: &[Violation],
    warnings: &[penv_dotenv::Warning],
    drift: &[String],
) -> Value {
    json!({
        "schema": show(schema_path),
        "env": env_path,
        "ok": diagnostics.is_empty() && violations.is_empty(),
        "diagnostics": diagnostics.iter().map(Diagnostic::to_json).collect::<Vec<_>>(),
        "violations": violations.iter().map(Violation::to_json).collect::<Vec<_>>(),
        "warnings": warnings.iter().map(|w| json!({
            "line": w.line,
            "code": w.code,
            "message": w.message,
        })).collect::<Vec<_>>(),
        "drift": drift.iter().map(|key| json!({
            "key": key,
            "code": "drift",
            "message": format!("{key} has a value with no block in the schema. penv masks it, and validates nothing about it. Add a block, or drop the key."),
        })).collect::<Vec<_>>(),
    })
}
