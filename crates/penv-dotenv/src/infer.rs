use penv_schema::{
    BaseType, Key, Schema, Type, is_absolute_url, is_email, is_public_prefixed, parse_boolean,
};

use crate::read::Dotenv;

/// Draft a schema from a `.env`. Every key is sensitive and required unless a
/// bundler prefix or a value too dull to be a secret says otherwise.
pub fn infer(env: &Dotenv) -> Schema {
    let mut schema = Schema::default();
    for entry in &env.entries {
        let prefixed = is_public_prefixed(&entry.key);
        let ty = infer_type(&entry.key, &entry.value);
        let copied = !entry.value.is_empty() && (prefixed || is_dull(&entry.value));
        let default = copied.then(|| entry.value.clone());
        let sensitive = !prefixed && !copied;
        // Write @sensitive only where the prefix rule alone would not reach the same answer.
        let inferred_sensitive = !prefixed;
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

/// A value the committed schema may carry: nothing here can be a credential.
fn is_dull(value: &str) -> bool {
    parse_boolean(value).is_some()
        || value.parse::<i64>().is_ok()
        || is_lowercase_word(value)
        || is_loopback_url(value)
}

fn is_lowercase_word(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_lowercase())
}

/// `http://localhost:3000` and nothing that could carry a credential in it.
fn is_loopback_url(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    if !matches!(scheme, "http" | "https") || rest.contains('?') {
        return false;
    }
    let authority = rest.split(['/', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return false;
    }
    let host = authority.rsplit_once(':').map_or(authority, |(h, _)| h);
    matches!(host, "localhost" | "127.0.0.1")
}

/// The type `init` reads out of one pair. `set` uses it for a key the schema
/// does not list yet, without ever copying the value.
pub fn infer_type(key: &str, value: &str) -> Type {
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
        let mut ty = Type::new(BaseType::Number);
        ty.constraints.push(("isInt".into(), "true".into()));
        return ty;
    }
    if matches!(value.parse::<f64>(), Ok(n) if n.is_finite()) {
        return Type::new(BaseType::Number);
    }
    if is_email(value) {
        return Type::new(BaseType::Email);
    }
    Type::new(BaseType::String)
}
