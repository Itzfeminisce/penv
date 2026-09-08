//! A structural JSON Schema check: enough of draft 2020-12 to hold the manifest
//! schema to its word without another dependency.

use serde_json::Value;

pub fn validate(schema: &Value, instance: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    check(schema, schema, instance, "$", &mut errors);
    errors
}

fn check(root: &Value, schema: &Value, value: &Value, path: &str, errors: &mut Vec<String>) {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        match resolve(root, reference) {
            Some(target) => check(root, target, value, path, errors),
            None => errors.push(format!("{path}: {reference} does not resolve")),
        }
        return;
    }

    if let Some(expected) = schema.get("type")
        && !type_matches(expected, value)
    {
        errors.push(format!(
            "{path}: expected {expected}, found {}",
            kind(value)
        ));
        return;
    }

    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if value.get(name).is_none() {
                errors.push(format!("{path}: {name} is missing"));
            }
        }
    }

    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        if schema.get("additionalProperties") == Some(&Value::Bool(false))
            && let Some(object) = value.as_object()
        {
            for name in object.keys() {
                if !properties.contains_key(name) {
                    errors.push(format!("{path}: {name} is not in the schema"));
                }
            }
        }
        for (name, subschema) in properties {
            if let Some(child) = value.get(name) {
                check(root, subschema, child, &format!("{path}.{name}"), errors);
            }
        }
    }

    if let Some(items) = schema.get("items")
        && let Some(array) = value.as_array()
    {
        for (index, child) in array.iter().enumerate() {
            check(root, items, child, &format!("{path}[{index}]"), errors);
        }
    }
}

fn resolve<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    reference
        .strip_prefix("#/")?
        .split('/')
        .try_fold(root, |node, segment| node.get(segment))
}

fn type_matches(expected: &Value, value: &Value) -> bool {
    match expected {
        Value::String(name) => is_type(name, value),
        Value::Array(names) => names
            .iter()
            .filter_map(Value::as_str)
            .any(|name| is_type(name, value)),
        _ => false,
    }
}

fn is_type(name: &str, value: &Value) -> bool {
    match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
