use std::path::{Path, PathBuf};

use penv_guards::{Guard, Roots, Scope, Write};
use serde_json::{Value, json};

use crate::claim;
use crate::commands::init::yes_no;
use crate::commands::load_schema;
use crate::error::{CliError, Exit};
use crate::files::{Disk, home, on_path, show, write_file_making_parents};
use crate::output::{Output, Report, table};

/// Native Windows runs Claude Code without a sandbox, so the deny rules and the
/// hook are all there is.
#[cfg(windows)]
const PLATFORM_NOTE: Option<&str> =
    Some("native Windows has no Claude Code sandbox; the deny rules and the hook still apply");
#[cfg(not(windows))]
const PLATFORM_NOTE: Option<&str> = None;

/// Write what every installed harness enforces, or report where that stands.
pub fn run(
    out: &Output,
    cwd: &Path,
    check: bool,
    all: bool,
    named: &[String],
) -> Result<Report, CliError> {
    let (schema_path, schema) = load_schema(cwd)?;
    let dir = schema_path.parent().unwrap_or(cwd).to_path_buf();
    let roots = Roots::new(show(&dir), home());
    let probe = Installed {
        repo: dir.clone(),
        home: home().map(PathBuf::from),
    };

    let guards = penv_guards::available(&Disk, &roots);
    for name in named {
        if !guards.iter().any(|g| &g.name == name) {
            return Err(CliError::new(
                "unknown_harness",
                format!("penv has no guard for {name}."),
                "Run penv guard --check to see the harnesses it knows.",
            ));
        }
    }

    let json = schema.to_json();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut harnesses: Vec<Value> = Vec::new();
    let mut blocks: Vec<Value> = Vec::new();
    let mut failed = false;

    for guard in &guards {
        let installed = penv_guards::is_installed(guard, &probe);
        // A check reports on every harness; a write only touches the ones asked for.
        let selected = if !named.is_empty() {
            named.iter().any(|n| n == &guard.name)
        } else {
            all || installed || check
        };

        let mut files: Vec<Value> = Vec::new();
        for entry in guard.writes.iter().filter(|w| w.scope == Scope::Project) {
            let path = dir.join(&entry.path);
            let status = if !selected {
                "skipped".to_string()
            } else {
                act(guard, entry, &path, &json, check)?
            };
            failed |= installed && (status == "missing" || status == "stale");
            rows.push(vec![
                guard.name.clone(),
                yes_no(installed),
                entry.path.clone(),
                status.clone(),
            ]);
            files.push(json!({ "path": entry.path, "status": status }));
        }

        if selected && !check {
            for entry in guard.writes.iter().filter(|w| w.scope == Scope::User) {
                blocks.push(json!({
                    "harness": guard.name,
                    "path": entry.path,
                    "content": pretty(&render(guard, entry, &json)?),
                }));
            }
        }

        harnesses.push(json!({
            "name": guard.name,
            "description": guard.description,
            "installed": installed,
            "selected": selected,
            "files": files,
        }));
    }

    let claim = claim::for_schema(&schema);
    let style = out.style();
    let mut lines = vec![table(
        &["HARNESS", "INSTALLED", "FILE", "STATUS"],
        &rows,
        &style,
    )];
    if !blocks.is_empty() {
        lines.push(String::new());
        for block in &blocks {
            lines.push(style.dim(&format!(
                "paste this into {} yourself; penv does not write outside the repository",
                block["path"].as_str().unwrap_or_default()
            )));
            lines.push(block["content"].as_str().unwrap_or_default().to_string());
        }
    }
    if let Some(note) = PLATFORM_NOTE {
        lines.push(String::new());
        lines.push(style.dim(note));
    }
    lines.push(String::new());
    lines.push(style.dim(claim));

    let report = Report::new(
        json!({
            "schema": show(&schema_path),
            "mode": if check { "check" } else { "write" },
            "harnesses": harnesses,
            "userBlocks": blocks,
            "platformNote": PLATFORM_NOTE,
            "claim": claim,
        }),
        lines.join("\n"),
    );
    Ok(if check && failed {
        report.with_exit(Exit::Validation)
    } else {
        report
    })
}

/// Every installed harness, written. `init` calls this; a harness whose config
/// cannot be merged is left alone rather than failing the import.
pub fn auto(dir: &Path, schema: &Value) -> Vec<PathBuf> {
    let roots = Roots::new(show(dir), home());
    let probe = Installed {
        repo: dir.to_path_buf(),
        home: home().map(PathBuf::from),
    };
    let mut written = Vec::new();
    for guard in penv_guards::available(&Disk, &roots) {
        if !penv_guards::is_installed(&guard, &probe) {
            continue;
        }
        for entry in guard.writes.iter().filter(|w| w.scope == Scope::Project) {
            let path = dir.join(&entry.path);
            let Ok(fragment) = render(&guard, entry, schema) else {
                continue;
            };
            let existing = std::fs::read_to_string(&path).ok();
            let Ok(outcome) = penv_guards::apply(entry, existing.as_deref(), &fragment) else {
                continue;
            };
            if outcome.changed && write_file_making_parents(&path, &outcome.content).is_ok() {
                written.push(path);
            }
        }
    }
    written
}

/// Merge one write, and put it on disk unless this is a check.
fn act(
    guard: &Guard,
    entry: &Write,
    path: &Path,
    schema: &Value,
    check: bool,
) -> Result<String, CliError> {
    let fragment = render(guard, entry, schema)?;
    let existing = std::fs::read_to_string(path).ok();
    let present = existing.is_some();
    let outcome = penv_guards::apply(entry, existing.as_deref(), &fragment).map_err(|e| {
        CliError::new(
            "guard_failed",
            e.to_string(),
            "Fix the file by hand, then run penv guard again.",
        )
        .with_exit(Exit::Validation)
    })?;

    if check {
        return Ok(match (present, outcome.changed) {
            (false, _) => "missing".into(),
            (true, true) => "stale".into(),
            (true, false) => "written".into(),
        });
    }
    if outcome.changed {
        write_file_making_parents(path, &outcome.content)?;
    }
    Ok(if outcome.changed {
        "written"
    } else {
        "current"
    }
    .into())
}

fn render(guard: &Guard, entry: &Write, schema: &Value) -> Result<String, CliError> {
    penv_guards::render(guard, entry, schema, env!("CARGO_PKG_VERSION")).map_err(|e| {
        CliError::new(
            "guard_failed",
            e.to_string(),
            "Fix the guard folder, or drop it so the built-in one is used again.",
        )
        .with_exit(Exit::Validation)
    })
}

fn pretty(rendered: &str) -> String {
    serde_json::from_str::<Value>(rendered)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| rendered.to_string())
}

struct Installed {
    repo: PathBuf,
    home: Option<PathBuf>,
}

impl penv_guards::Probe for Installed {
    fn exists(&self, path: &str) -> bool {
        match path.strip_prefix("~/") {
            Some(rest) => self.home.as_ref().is_some_and(|h| h.join(rest).exists()),
            None => self.repo.join(path).exists(),
        }
    }

    fn on_path(&self, exe: &str) -> bool {
        on_path(exe)
    }
}
