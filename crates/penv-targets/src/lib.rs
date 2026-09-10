//! Language target loading and rendering over the schema JSON.
//!
//! A target is a folder: `target.toml` and `env.tmpl`. The same layout is read
//! from the repository, from the home directory and from inside the binary, so
//! adding a language touches no Rust. [`folder`] is that lookup, shared with the
//! harness guards.

mod detect;
mod error;
pub mod folder;
mod import;
mod load;
mod remember;
mod render;
mod target;

pub use detect::{candidates, layout_output, layout_root, output_path, package_of, suggested};
pub use error::Error;
pub use folder::{BuiltIn, Roots, Source, Tree};
pub use import::{Config, extends_of, import_line, join};
pub use load::{BUILT_IN, available, detected, load};
pub use remember::{MARK, OptionValue, hand_written, override_body};
pub use render::render;
pub use target::{BASE_TYPES, Check, INT_TYPE, Knob, Layout, Rule, Suggest, Target, word};
