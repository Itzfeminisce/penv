use std::path::{Path, PathBuf};
use std::process::Command;

use penv_schema::Schema;
use penv_targets::{Roots, Target};
use serde_json::{Value, json};

use crate::commands::init::yes_no;
use crate::commands::load_schema;
use crate::error::{CliError, Exit};
use crate::files::{Disk, home, read_file, show, write_file_making_parents};
use crate::output::{Output, Report, table};

/// Write the typed file for one target, or list the targets there are.
pub fn run(
    out: &Output,
    cwd: &Path,
    target: Option<&str>,
    to: Option<&Path>,
    check: bool,
) -> Result<Report, CliError> {
    let (schema_path, schema) = load_schema(cwd)?;
    let dir = schema_path.parent().unwrap_or(cwd).to_path_buf();
    let roots = roots(&dir);

    match target {
        None => Ok(list(out, &dir, &roots)),
        Some(name) => {
            let target = penv_targets::load(&Disk, &roots, name).map_err(refused)?;
            let rendered =
                penv_targets::render(&target, &schema.to_json(), version()).map_err(refused)?;
            let path = to
                .map(Path::to_path_buf)
                .unwrap_or_else(|| dir.join(&target.output));
            if check {
                verify(out, &target, &path, &rendered)
            } else {
                write(out, &target, &path, &rendered)
            }
        }
    }
}

/// Every target the repository asks for, written, whether it is built in or a
/// folder someone dropped in. `init` calls this; a target that fails to render is
/// skipped rather than failing the import.
pub fn auto(dir: &Path, schema: &Schema) -> Vec<PathBuf> {
    let roots = roots(dir);
    let json = schema.to_json();
    let mut written = Vec::new();
    for target in penv_targets::available(&Disk, &roots) {
        if !penv_targets::detected(&Disk, &roots, &target) {
            continue;
        }
        let Ok(rendered) = penv_targets::render(&target, &json, version()) else {
            continue;
        };
        let path = dir.join(&target.output);
        if write_file_making_parents(&path, &rendered).is_ok() {
            written.push(path);
        }
    }
    written
}

fn roots(dir: &Path) -> Roots {
    Roots::new(show(dir), home())
}

fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn refused(error: penv_targets::Error) -> CliError {
    match error {
        penv_targets::Error::NotFound { .. } => CliError::new(
            "unknown_target",
            error.to_string(),
            "Run penv gen with no target to see the ones this repository has.",
        ),
        _ => CliError::new(
            "target_failed",
            error.to_string(),
            "Fix the target folder, or drop it so the built-in one is used again.",
        )
        .with_exit(Exit::Validation),
    }
}

fn list(out: &Output, dir: &Path, roots: &Roots) -> Report {
    let targets = penv_targets::available(&Disk, roots);
    let rows: Vec<Vec<String>> = targets
        .iter()
        .map(|target| {
            vec![
                target.name.clone(),
                target.source.as_str().to_string(),
                target.output.clone(),
                yes_no(penv_targets::detected(&Disk, roots, target)),
            ]
        })
        .collect();

    let style = out.style();
    let text = format!(
        "{}\n\n{}",
        table(&["NAME", "SOURCE", "OUTPUT", "DETECTED"], &rows, &style),
        style.dim("penv gen <name> writes one of these")
    );

    Report::new(
        json!({
            "schema": show(&dir.join(crate::files::SCHEMA_FILE)),
            "targets": targets.iter().map(|target| json!({
                "name": target.name,
                "source": target.source.as_str(),
                "dir": target.dir,
                "output": target.output,
                "detect": target.detect,
                "detected": penv_targets::detected(&Disk, roots, target),
            })).collect::<Vec<_>>(),
        }),
        text,
    )
}

fn write(out: &Output, target: &Target, path: &Path, rendered: &str) -> Result<Report, CliError> {
    let unchanged = read_file(path).is_ok_and(|existing| existing == rendered);
    if !unchanged {
        write_file_making_parents(path, rendered)?;
    }
    let style = out.style();
    let text = format!(
        "{} {} {}",
        style.green(if unchanged { "same" } else { "wrote" }),
        show(path),
        style.dim(&format!("from the {} target", target.name))
    );
    Ok(Report::new(
        json!({
            "target": target.name,
            "source": target.source.as_str(),
            "output": show(path),
            "changed": !unchanged,
        }),
        text,
    ))
}

fn verify(out: &Output, target: &Target, path: &Path, rendered: &str) -> Result<Report, CliError> {
    let status = match std::fs::read_to_string(path) {
        Err(_) => "missing",
        Ok(existing) if existing == rendered => "current",
        Ok(_) => "stale",
    };
    let compiled = compile(target, rendered);

    let style = out.style();
    let ok = status == "current" && compiled["status"] != "failed";
    let mut lines = vec![format!(
        "{} {} {}",
        if ok {
            style.green(status)
        } else {
            style.red(status)
        },
        show(path),
        style.dim(&format!("from the {} target", target.name))
    )];
    lines.push(style.dim(&format!(
        "{} {}",
        compiled["tool"].as_str().unwrap_or("no toolchain"),
        compiled["detail"].as_str().unwrap_or_default()
    )));

    let report = Report::new(
        json!({
            "target": target.name,
            "output": show(path),
            "status": status,
            "compile": compiled,
        }),
        lines.join("\n"),
    );
    Ok(if ok {
        report
    } else {
        report.with_exit(Exit::Validation)
    })
}

/// Run the target's own `[check]` command over what was rendered. A missing
/// toolchain is a skip: `gen --check` is not a reason to install one.
fn compile(target: &Target, rendered: &str) -> Value {
    let Some(check) = &target.check else {
        return json!({ "tool": null, "status": "skipped", "detail": "this target declares no [check] command" });
    };
    let Some(tool) = check.command.first() else {
        return json!({ "tool": null, "status": "skipped", "detail": "this target declares no [check] command" });
    };
    if !probed(&check.probe) {
        return json!({
            "tool": tool,
            "status": "skipped",
            "detail": format!("{} is not on PATH", check.probe.first().unwrap_or(tool)),
        });
    }

    let dir = std::env::temp_dir().join(format!("penv-gen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return json!({ "tool": tool, "status": "skipped", "detail": "no writable temp directory" });
    }
    let file = source_name(target);
    let outcome = write_all(&dir, check, &file, rendered).and_then(|()| {
        let arguments: Vec<String> = check.command[1..]
            .iter()
            .map(|argument| argument.replace("{file}", &file))
            .collect();
        output(Command::new(tool).current_dir(&dir).args(arguments))
    });
    let _ = std::fs::remove_dir_all(&dir);

    match outcome {
        Ok(()) => {
            json!({ "tool": tool, "status": "ok", "detail": format!("{tool} accepted the output") })
        }
        Err(detail) => json!({ "tool": tool, "status": "failed", "detail": detail }),
    }
}

fn write_all(
    dir: &Path,
    check: &penv_targets::Check,
    file: &str,
    rendered: &str,
) -> Result<(), String> {
    std::fs::write(dir.join(file), rendered).map_err(|e| e.to_string())?;
    for (name, body) in &check.files {
        std::fs::write(dir.join(name), body).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The name the check command compiles, taken from where the target writes.
fn source_name(target: &Target) -> String {
    Path::new(&target.output)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| target.output.clone())
}

fn output(command: &mut Command) -> Result<(), String> {
    match command.output() {
        Err(e) => Err(e.to_string()),
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stderr);
            let text = if text.trim().is_empty() {
                String::from_utf8_lossy(&out.stdout).into_owned()
            } else {
                text.into_owned()
            };
            Err(text.lines().take(5).collect::<Vec<_>>().join(" "))
        }
    }
}

/// The target's probe has to answer, so a shim that only prints an install
/// prompt counts as absent. No probe means the command speaks for itself.
fn probed(probe: &[String]) -> bool {
    let Some(tool) = probe.first() else {
        return true;
    };
    Command::new(tool)
        .args(&probe[1..])
        .output()
        .is_ok_and(|out| out.status.success())
}
