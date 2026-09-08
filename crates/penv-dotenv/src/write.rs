use std::fmt::Write as _;

use penv_schema::is_valid_key_name;
use thiserror::Error;

/// A pair that cannot be written in the safe subset. Each names the key and the fix.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WriteError {
    #[error("{key} is not a usable key name. Use upper snake case, such as DATABASE_URL.")]
    InvalidKey { key: String },
    #[error("{key} is written twice. Keep one value per key.")]
    DuplicateKey { key: String },
    #[error("{key} spans more than one line. A .env value is a single line.")]
    MultiLine { key: String },
    #[error("{key} contains a $. penv never expands values, so store the expanded value instead.")]
    Interpolation { key: String },
    #[error("{key} contains both quote characters, so it cannot be quoted without escapes.")]
    Unquotable { key: String },
}

/// Write the safe subset only: UTF-8, LF, no comments, quotes only where needed.
pub fn write(entries: &[(&str, &str)]) -> Result<String, WriteError> {
    let mut out = String::new();
    let mut seen: Vec<&str> = Vec::new();
    for (key, value) in entries {
        let key = *key;
        if !is_valid_key_name(key) || key != key.to_ascii_uppercase() {
            return Err(WriteError::InvalidKey {
                key: key.to_string(),
            });
        }
        if seen.contains(&key) {
            return Err(WriteError::DuplicateKey {
                key: key.to_string(),
            });
        }
        seen.push(key);
        if value.contains(['\n', '\r']) {
            return Err(WriteError::MultiLine {
                key: key.to_string(),
            });
        }
        if value.contains('$') {
            return Err(WriteError::Interpolation {
                key: key.to_string(),
            });
        }
        let _ = writeln!(out, "{key}={}", quote(key, value)?);
    }
    Ok(out)
}

fn quote(key: &str, value: &str) -> Result<String, WriteError> {
    let double = value.contains('"');
    let single = value.contains('\'');
    let needs = value.contains([' ', '\t', '#']) || double || single;
    if !needs {
        return Ok(value.to_string());
    }
    if double && single {
        return Err(WriteError::Unquotable {
            key: key.to_string(),
        });
    }
    if double {
        Ok(format!("'{value}'"))
    } else {
        Ok(format!("\"{value}\""))
    }
}
