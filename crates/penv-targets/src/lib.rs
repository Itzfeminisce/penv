//! Language target loading and rendering over the schema JSON.
//!
//! A target is a folder: `target.toml` and `env.tmpl`. The same layout is read
//! from the repository, from the home directory and from inside the binary, so
//! adding a language touches no Rust.

mod error;
mod load;
mod render;
mod target;

pub use error::Error;
pub use load::{BUILT_IN, BuiltIn, Roots, Tree, available, detected, load};
pub use render::render;
pub use target::{BASE_TYPES, Source, Target};
