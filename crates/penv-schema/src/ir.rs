use std::fmt;

use serde_json::{Value, json};

/// Key prefixes that a bundler inlines into client code, so the value is public.
pub const PUBLIC_PREFIXES: [&str; 6] = [
    "NEXT_PUBLIC_",
    "VITE_",
    "PUBLIC_",
    "EXPO_PUBLIC_",
    "NUXT_PUBLIC_",
    "REACT_APP_",
];

pub fn is_public_prefixed(name: &str) -> bool {
    PUBLIC_PREFIXES.iter().any(|p| name.starts_with(p))
}

pub fn is_valid_key_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The grammar version this build writes and understands.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    pub org: Option<String>,
    pub project: Option<String>,
    pub schema_version: u32,
    pub default_sensitive: bool,
    pub default_required: bool,
    pub keys: Vec<Key>,
}

impl Default for Schema {
    fn default() -> Self {
        Schema {
            org: None,
            project: None,
            schema_version: SCHEMA_VERSION,
            default_sensitive: true,
            default_required: true,
            keys: Vec::new(),
        }
    }
}

impl Schema {
    pub fn get(&self, name: &str) -> Option<&Key> {
        self.keys.iter().find(|k| k.name == name)
    }

    /// True when the file names a cloud project.
    pub fn is_cloud(&self) -> bool {
        self.org.is_some() && self.project.is_some()
    }

    pub fn to_json(&self) -> Value {
        json!({
            "schemaVersion": self.schema_version,
            "org": self.org,
            "project": self.project,
            "defaultSensitive": self.default_sensitive,
            "defaultRequired": self.default_required,
            "keys": self.keys.iter().map(Key::to_json).collect::<Vec<_>>(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Key {
    pub name: String,
    pub description: Option<String>,
    pub ty: Type,
    pub required: bool,
    /// `@required` / `@optional` when written; `None` means required was inferred.
    pub required_decorator: Option<bool>,
    pub sensitive: bool,
    /// `@sensitive` / `@sensitive=false` when written.
    pub sensitive_decorator: Option<bool>,
    pub default: Option<String>,
    pub example: Option<String>,
    pub docs: Option<String>,
    pub since: Option<String>,
    pub deprecated: Option<String>,
    pub rotate: Option<String>,
    /// The spec's `@dynamic` / `@static` pair: preserved, never acted on.
    pub dynamic: Option<bool>,
    /// penv's own marker: the engine the cloud resolves the value from.
    pub dynamic_from: Option<String>,
}

impl Key {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "type": self.ty.to_json(),
            "required": self.required,
            "sensitive": self.sensitive,
            "default": self.default,
            "example": self.example,
            "docs": self.docs,
            "since": self.since,
            "deprecated": self.deprecated,
            "rotate": self.rotate,
            "dynamic": self.dynamic,
            "dynamicFrom": self.dynamic_from,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BaseType {
    #[default]
    String,
    Number,
    Boolean,
    Url,
    Email,
    Port,
    Enum,
}

impl BaseType {
    pub fn as_str(&self) -> &'static str {
        match self {
            BaseType::String => "string",
            BaseType::Number => "number",
            BaseType::Boolean => "boolean",
            BaseType::Url => "url",
            BaseType::Email => "email",
            BaseType::Port => "port",
            BaseType::Enum => "enum",
        }
    }

    pub fn from_name(s: &str) -> Option<BaseType> {
        Some(match s {
            "string" => BaseType::String,
            "number" => BaseType::Number,
            "boolean" => BaseType::Boolean,
            "url" => BaseType::Url,
            "email" => BaseType::Email,
            "port" => BaseType::Port,
            "enum" => BaseType::Enum,
            _ => return None,
        })
    }

    /// Constraint names this type accepts inside its call parentheses.
    pub fn constraints(&self) -> &'static [&'static str] {
        match self {
            BaseType::String => &[
                "startsWith",
                "endsWith",
                "minLength",
                "maxLength",
                "matches",
            ],
            BaseType::Number => &["min", "max", "isInt", "precision"],
            BaseType::Port => &["min", "max"],
            _ => &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Type {
    pub base: BaseType,
    /// Enum members, in the order written.
    pub members: Vec<String>,
    /// Named constraints, sorted by name so rendering is canonical.
    pub constraints: Vec<(String, String)>,
}

impl Type {
    pub fn new(base: BaseType) -> Type {
        Type {
            base,
            members: Vec::new(),
            constraints: Vec::new(),
        }
    }

    pub fn constraint(&self, name: &str) -> Option<&str> {
        self.constraints
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn to_json(&self) -> Value {
        let constraints: serde_json::Map<String, Value> = self
            .constraints
            .iter()
            .map(|(n, v)| (n.clone(), Value::String(v.clone())))
            .collect();
        json!({
            "name": self.base.as_str(),
            "raw": self.to_string(),
            "members": self.members,
            "constraints": constraints,
        })
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.base.as_str())?;
        if self.members.is_empty() && self.constraints.is_empty() {
            return Ok(());
        }
        let mut args: Vec<String> = self.members.iter().map(|m| quote(m)).collect();
        args.extend(
            self.constraints
                .iter()
                .map(|(n, v)| format!("{n}={}", quote(v))),
        );
        write!(f, "({})", args.join(","))
    }
}

/// The one quoting rule: decorator values, type arguments and defaults all use it.
pub(crate) fn quote(v: &str) -> String {
    if v.is_empty() || v.contains([' ', '\t', ',', '(', ')', '#', '"', '\'']) {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        v.to_string()
    }
}

/// A problem in the schema file itself, located for the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: u32,
    pub column: u32,
    pub code: String,
    pub message: String,
}

impl Diagnostic {
    pub fn new(line: u32, column: u32, code: &str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            line,
            column,
            code: code.to_string(),
            message: message.into(),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "line": self.line,
            "column": self.column,
            "code": self.code,
            "message": self.message,
        })
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{} {}", self.line, self.column, self.message)
    }
}
