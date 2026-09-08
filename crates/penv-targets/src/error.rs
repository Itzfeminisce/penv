use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No folder for this name in the repo, the home directory or the binary.
    NotFound {
        name: String,
        looked: Vec<String>,
    },
    Malformed {
        dir: String,
        message: String,
    },
    /// The `[types]` map has no entry for a base type the schema uses.
    NoTypeFor {
        target: String,
        base: String,
    },
    Render {
        target: String,
        message: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound { name, looked } => {
                write!(f, "no target named {name}; looked in {}", looked.join(", "))
            }
            Error::Malformed { dir, message } => write!(f, "{dir} is not a target: {message}"),
            Error::NoTypeFor { target, base } => {
                write!(f, "target {target} has no [types] entry for {base}")
            }
            Error::Render { target, message } => {
                write!(f, "target {target} failed to render: {message}")
            }
        }
    }
}

impl std::error::Error for Error {}
