use penv_schema::{
    BaseType, Key, Schema, Type, is_absolute_url, is_email, is_public_prefixed, parse_boolean,
};

use crate::read::Dotenv;

/// Name fragments that make a key sensitive whatever its value looks like.
const SENSITIVE_FRAGMENTS: [&str; 7] = [
    "KEY",
    "SECRET",
    "TOKEN",
    "PASSWORD",
    "PRIVATE",
    "DSN",
    "CREDENTIAL",
];

/// Vendor prefixes that mark a value as issued credential material.
const CREDENTIAL_PREFIXES: [&str; 10] = [
    "sk_",
    "rk_",
    "xoxb-",
    "xoxp-",
    "ghp_",
    "gho_",
    "github_pat_",
    "AKIA",
    "AIza",
    "-----BEGIN",
];

/// Draft a schema from a `.env`. Sensitive values are never carried into it.
pub fn infer(env: &Dotenv) -> Schema {
    let mut schema = Schema::default();
    for entry in &env.entries {
        let sensitive = is_sensitive(&entry.key, &entry.value);
        let ty = infer_type(&entry.key, &entry.value);
        let default = if sensitive || entry.value.is_empty() {
            None
        } else {
            Some(entry.value.clone())
        };
        // Write @sensitive only where the prefix rule alone would not reach the same answer.
        let inferred_sensitive = !is_public_prefixed(&entry.key);
        schema.keys.push(Key {
            name: entry.key.clone(),
            ty,
            required: default.is_none(),
            sensitive,
            sensitive_decorator: (sensitive != inferred_sensitive).then_some(sensitive),
            default,
            ..Key::default()
        });
    }
    schema
}

fn is_sensitive(key: &str, value: &str) -> bool {
    if is_public_prefixed(key) {
        return false;
    }
    let name = key.to_ascii_uppercase();
    SENSITIVE_FRAGMENTS.iter().any(|f| name.contains(f)) || looks_like_credential(value)
}

fn looks_like_credential(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    if CREDENTIAL_PREFIXES.iter().any(|p| value.starts_with(p)) {
        return true;
    }
    is_jwt(value) || is_long_opaque(value) || has_userinfo(value)
}

/// A connection string that carries its own password, such as a database URL.
fn has_userinfo(value: &str) -> bool {
    let Some((_, rest)) = value.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    authority
        .rsplit_once('@')
        .is_some_and(|(userinfo, _)| userinfo.contains(':'))
}

fn is_jwt(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 3
        && value.starts_with("eyJ")
        && parts.iter().all(|p| {
            p.len() >= 8
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

/// A long unbroken token with mixed case and digits reads as a generated secret.
fn is_long_opaque(value: &str) -> bool {
    value.len() >= 32
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_' | '-'))
        && value.chars().any(|c| c.is_ascii_digit())
        && value.chars().any(|c| c.is_ascii_lowercase())
        && value.chars().any(|c| c.is_ascii_uppercase())
}

fn infer_type(key: &str, value: &str) -> Type {
    if value.is_empty() {
        return Type::new(if key.ends_with("PORT") {
            BaseType::Port
        } else {
            BaseType::String
        });
    }
    if is_absolute_url(value) {
        return Type::new(BaseType::Url);
    }
    if key.ends_with("PORT") && matches!(value.parse::<u32>(), Ok(n) if (1..=65535).contains(&n)) {
        return Type::new(BaseType::Port);
    }
    if parse_boolean(value).is_some() {
        return Type::new(BaseType::Boolean);
    }
    if value.parse::<i64>().is_ok() {
        return Type::new(BaseType::Integer);
    }
    if matches!(value.parse::<f64>(), Ok(n) if n.is_finite()) {
        return Type::new(BaseType::Number);
    }
    if is_email(value) {
        return Type::new(BaseType::Email);
    }
    Type::new(BaseType::String)
}
