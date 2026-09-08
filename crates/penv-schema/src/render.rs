use std::fmt::Write as _;

use crate::ir::{Key, Schema, quote};

/// Write a schema back out canonically. Parsing the result yields an equal `Schema`.
pub fn render(schema: &Schema) -> String {
    let mut out = String::new();
    let mut header = String::from("#");
    if let (Some(org), Some(project)) = (&schema.org, &schema.project) {
        let _ = write!(header, " @penv={org}/{project}");
    }
    let _ = write!(header, " @schema={}", schema.schema_version);
    out.push_str(&header);
    out.push('\n');
    if !schema.default_sensitive {
        out.push_str("# @defaultSensitive=false\n");
    }
    if !schema.default_required {
        out.push_str("# @defaultRequired=false\n");
    }

    for key in &schema.keys {
        out.push('\n');
        out.push_str(&render_key(key));
    }
    out
}

/// One key's block, the way `render` writes it. `set` appends it to a file it
/// must not otherwise reformat.
pub fn render_key(key: &Key) -> String {
    let mut out = String::new();
    let out = &mut out;
    if let Some(description) = &key.description {
        let _ = writeln!(out, "# {description}");
    }

    let mut decorators = vec![format!("@type={}", key.ty)];
    match key.required_decorator {
        Some(true) => decorators.push("@required".into()),
        Some(false) => decorators.push("@optional".into()),
        None => {}
    }
    match key.sensitive_decorator {
        Some(true) => decorators.push("@sensitive".into()),
        Some(false) => decorators.push("@sensitive=false".into()),
        None => {}
    }
    if let Some(v) = &key.example {
        decorators.push(format!("@example={}", quote(v)));
    }
    if let Some(v) = &key.docs {
        decorators.push(format!("@docs={}", quote(v)));
    }
    if let Some(v) = &key.since {
        decorators.push(format!("@since={}", quote(v)));
    }
    if let Some(v) = &key.deprecated {
        if v.is_empty() {
            decorators.push("@deprecated".into());
        } else {
            decorators.push(format!("@deprecated={}", quote(v)));
        }
    }
    if let Some(v) = &key.rotate {
        decorators.push(format!("@rotate={v}"));
    }
    match key.dynamic {
        Some(true) => decorators.push("@dynamic".into()),
        Some(false) => decorators.push("@static".into()),
        None => {}
    }
    if let Some(v) = &key.dynamic_from {
        decorators.push(format!("@dynamicFrom={}", quote(v)));
    }

    let _ = writeln!(out, "# {}", decorators.join(" "));
    let _ = writeln!(
        out,
        "{}={}",
        key.name,
        key.default.as_deref().map(quote).unwrap_or_default()
    );
    std::mem::take(out)
}
