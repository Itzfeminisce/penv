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

pub fn show(path: &Path) -> String {
    path.display().to_string()
}
