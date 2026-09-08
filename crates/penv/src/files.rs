use std::path::{Path, PathBuf};

use crate::error::CliError;

pub const SCHEMA_FILE: &str = ".env.schema";
pub const ENV_FILE: &str = ".env";
pub const GITIGNORE_FILE: &str = ".gitignore";

/// The nearest `.env.schema` at or above `start`. A monorepo holds one per app.
pub fn find_schema(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|dir| {
        let candidate = dir.join(SCHEMA_FILE);
        candidate.is_file().then_some(candidate)
    })
}

pub fn read_file(path: &Path) -> Result<String, CliError> {
    std::fs::read_to_string(path).map_err(|e| {
        CliError::new(
            "unreadable_file",
            format!("{} could not be read: {e}.", show(path)),
            "Check the path and its permissions.",
        )
    })
}

pub fn write_file(path: &Path, contents: &str) -> Result<(), CliError> {
    std::fs::write(path, contents).map_err(|e| {
        CliError::new(
            "unwritable_file",
            format!("{} could not be written: {e}.", show(path)),
            "Check the directory and its permissions.",
        )
    })
}

pub fn write_file_making_parents(path: &Path, contents: &str) -> Result<(), CliError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| {
            CliError::new(
                "unwritable_file",
                format!("{} could not be created: {e}.", show(parent)),
                "Check the directory and its permissions.",
            )
        })?;
    }
    write_file(path, contents)
}

/// A path in one separator, the platform's. Joining a `.claude/settings.json`
/// onto a Windows directory otherwise prints both.
pub fn show(path: &Path) -> String {
    let text = path.display().to_string();
    if std::path::MAIN_SEPARATOR == '\\' {
        text.replace('/', "\\")
    } else {
        text
    }
}

/// A hook script the harness runs itself. A file without the execute bit fails
/// open, so the mode is part of writing it.
pub fn write_executable(path: &Path, contents: &str) -> Result<(), CliError> {
    write_file_making_parents(path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|e| {
            CliError::new(
                "unwritable_file",
                format!("{} could not be made executable: {e}.", show(path)),
                "Check the file and its permissions.",
            )
        })?;
    }
    Ok(())
}

/// The home directory, for the `~/.penv` half of every lookup order.
pub fn home() -> Option<String> {
    for var in ["HOME", "USERPROFILE"] {
        if let Some(value) = std::env::var_os(var)
            && !value.is_empty()
        {
            return Some(value.to_string_lossy().into_owned());
        }
    }
    None
}

/// True when the name resolves to something runnable on PATH.
pub fn on_path(exe: &str) -> bool {
    let extensions: Vec<String> = match std::env::var("PATHEXT") {
        Ok(list) if !list.is_empty() => list.split(';').map(str::to_lowercase).collect(),
        _ => vec![String::new()],
    };
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path).any(|dir| {
                extensions
                    .iter()
                    .any(|ext| dir.join(format!("{exe}{ext}")).is_file())
            })
        })
        .unwrap_or(false)
}

/// The filesystem, for the one tree the target and guard loaders read.
pub struct Disk;

impl penv_targets::Tree for Disk {
    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn dirs(&self, path: &str) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        out.sort();
        out
    }

    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shown_path_never_mixes_separators() {
        let shown = show(&PathBuf::from("repo").join(".claude/settings.json"));
        assert!(!(shown.contains('/') && shown.contains('\\')), "{shown}");
        assert!(shown.contains(std::path::MAIN_SEPARATOR), "{shown}");
    }

    #[cfg(unix)]
    #[test]
    fn a_hook_script_is_written_with_the_execute_bit() {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!("penv-hook-{}", std::process::id()));
        write_executable(&path, "#!/bin/sh\nexec penv hook cline \"$@\"\n").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        let _ = std::fs::remove_file(&path);
        assert_eq!(mode & 0o111, 0o111, "a script nobody can run fails open");
    }
}
