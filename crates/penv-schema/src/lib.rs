//! `.env.schema` as a value: parse it, project it to JSON, validate values against
//! it, and render it back. No I/O lives here.

mod ir;
mod parse;
mod render;
mod validate;

pub use ir::{
    BaseType, Diagnostic, Key, PUBLIC_PREFIXES, Schema, Type, is_public_prefixed, is_valid_key_name,
};
pub use parse::parse;
pub use render::render;
pub use validate::{
    Values, Violation, is_absolute_url, is_email, parse_boolean, validate, validate_key,
};
