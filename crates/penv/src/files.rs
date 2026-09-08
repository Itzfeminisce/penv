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

pub fn show(path: &Path) -> String {
    path.display().to_string()
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

/// The filesystem, for the target and guard loaders that read a tree.
pub struct Disk;

impl Disk {
    fn read(path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn dirs(path: &str) -> Vec<String> {
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
}

impl penv_targets::Tree for Disk {
    fn read(&self, path: &str) -> Option<String> {
        Disk::read(path)
    }
    fn dirs(&self, path: &str) -> Vec<String> {
        Disk::dirs(path)
    }
    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }
}

impl penv_guards::Tree for Disk {
    fn read(&self, path: &str) -> Option<String> {
        Disk::read(path)
    }
    fn dirs(&self, path: &str) -> Vec<String> {
        Disk::dirs(path)
    }
}
