//! The plain `.env` format: a reader tolerant of what real files contain, a writer
//! that emits only the safe subset, and inference of a schema draft from a file.

mod gitignore;
mod infer;
mod read;
mod write;

pub use gitignore::{GitignoreUpdate, IGNORE_LINES, ensure_ignored};
pub use infer::{infer, infer_type};
pub use read::{Dotenv, Entry, Warning, read};
pub use write::{WriteError, write};
