//! Harness guard loading and additive, non-weakening config merging.
//!
//! A guard is a folder: `guard.toml` and the templates it names. Merging only
//! ever adds; nothing an existing config says is removed or overwritten, so a
//! second run leaves the same bytes as the first.

mod error;
mod guard;
mod load;
mod merge;
mod render;

pub use error::Error;
pub use guard::{ENV_FILES, Format, Guard, Merge, Probe, Scope, Write, is_installed};
pub use load::{BUILT_IN, BuiltIn, Roots, Tree, available, load};
pub use merge::{Outcome, apply};
pub use render::render;
