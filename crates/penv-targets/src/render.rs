use minijinja::value::Value as Jinja;
use minijinja::{AutoEscape, Environment, UndefinedBehavior, context};
use serde_json::{Value, json};

use crate::error::Error;
use crate::target::Target;

/// A template sees the schema JSON, `penv.version`, and one computed field per
/// key: `lang_type`. Casting is the template's own business.
pub fn render(target: &Target, schema: &Value, penv_version: &str) -> Result<String, Error> {
    let env = environment();
    let mut context = schema.clone();
    let keys = context
        .get_mut("keys")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| Error::Render {
            target: target.name.clone(),
            message: "the schema JSON has no keys array".into(),
        })?;

    for key in keys.iter_mut() {
        let base = key["type"]["name"].as_str().unwrap_or("string").to_string();
        let lang = lang_type(&env, target, &base, &key["type"]["members"])?;
        key["lang_type"] = Value::String(lang);
    }
    context["penv"] = json!({ "version": penv_version });

    env.render_str(&target.template, Jinja::from_serialize(&context))
        .map_err(|e| Error::Render {
            target: target.name.clone(),
            message: describe(&e),
        })
}

fn lang_type(
    env: &Environment<'_>,
    target: &Target,
    base: &str,
    members: &Value,
) -> Result<String, Error> {
    let mapped = target.types.get(base).ok_or_else(|| Error::NoTypeFor {
        target: target.name.clone(),
        base: base.to_string(),
    })?;
    if base != "enum" {
        return Ok(mapped.clone());
    }
    let expression = env.compile_expression(mapped).map_err(|e| Error::Render {
        target: target.name.clone(),
        message: format!("the enum type expression is not valid: {}", describe(&e)),
    })?;
    expression
        .eval(context! { values => Jinja::from_serialize(members) })
        .map(|value| value.to_string())
        .map_err(|e| Error::Render {
            target: target.name.clone(),
            message: format!("the enum type expression failed: {}", describe(&e)),
        })
}

fn describe(error: &minijinja::Error) -> String {
    match error.detail() {
        Some(detail) => format!("{error}: {detail}"),
        None => error.to_string(),
    }
}

fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env.add_filter("pascal", pascal);
    env.add_filter("camel", camel);
    env.add_filter("snake", snake);
    env.add_filter("quote", quote);
    env.add_filter("json", to_json);
    env
}

/// Split a name on separators and on lower-to-upper humps, so `NEXT_PUBLIC_URL`
/// and `nextPublicUrl` produce the same words.
fn words(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut previous: Option<char> = None;
    for c in value.chars() {
        if !c.is_ascii_alphanumeric() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            previous = None;
            continue;
        }
        let hump = previous.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit())
            && c.is_ascii_uppercase();
        if hump && !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
        word.push(c);
        previous = Some(c);
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

fn pascal(value: &str) -> String {
    words(value)
        .iter()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_ascii_uppercase().to_string() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect()
}

fn camel(value: &str) -> String {
    let pascal = pascal(value);
    let mut chars = pascal.chars();
    match chars.next() {
        Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn snake(value: &str) -> String {
    words(value)
        .iter()
        .map(|word| word.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

fn quote(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\""))
}

fn to_json(value: Jinja) -> Result<String, minijinja::Error> {
    serde_json::to_string(&value).map_err(|e| {
        minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!("value cannot be written as JSON: {e}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::{Source, parse};
    use std::collections::BTreeMap;

    fn target(template: &str, enum_expression: &str) -> Target {
        let types: BTreeMap<String, String> = [
            ("string", "string"),
            ("number", "number"),
            ("integer", "number"),
            ("boolean", "boolean"),
            ("url", "string"),
            ("email", "string"),
            ("port", "number"),
            ("enum", enum_expression),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        Target {
            name: "fake".into(),
            output: "out".into(),
            detect: vec![],
            types,
            template: template.into(),
            source: Source::BuiltIn,
            dir: "built in".into(),
        }
    }

    fn schema(keys: Value) -> Value {
        json!({
            "schemaVersion": 1,
            "org": null,
            "project": null,
            "defaultSensitive": true,
            "defaultRequired": true,
            "keys": keys,
        })
    }

    fn key(name: &str, base: &str, members: Value) -> Value {
        json!({
            "name": name,
            "description": null,
            "type": { "name": base, "raw": base, "members": members, "constraints": {} },
            "required": true,
            "sensitive": true,
            "default": null,
            "example": null,
            "docs": null,
            "since": null,
            "deprecated": null,
            "rotate": null,
            "dynamic": null,
        })
    }

    fn rendered(template: &str, keys: Value) -> String {
        render(
            &target(template, "values | join(' | ')"),
            &schema(keys),
            "9.9.9",
        )
        .unwrap()
    }

    #[test]
    fn a_template_sees_the_version_and_the_schema() {
        let out = rendered(
            "{{ penv.version }} {{ schemaVersion }} {{ keys[0].name }}",
            json!([key("PORT", "port", json!([]))]),
        );
        assert_eq!(out, "9.9.9 1 PORT");
    }

    #[test]
    fn every_key_carries_the_language_type_the_map_names() {
        let out = rendered(
            "{% for key in keys %}{{ key.lang_type }};{% endfor %}",
            json!([
                key("PORT", "port", json!([])),
                key("HOST", "string", json!([])),
            ]),
        );
        assert_eq!(out, "number;string;");
    }

    #[test]
    fn the_enum_entry_is_an_expression_over_values() {
        let target = target(
            "{{ keys[0].lang_type }}",
            "values | map('quote') | join(' | ')",
        );
        let out = render(
            &target,
            &schema(json!([key(
                "NODE_ENV",
                "enum",
                json!(["development", "production"])
            )])),
            "9.9.9",
        )
        .unwrap();
        assert_eq!(out, "\"development\" | \"production\"");
    }

    #[test]
    fn a_broken_enum_expression_names_the_target() {
        let target = target("{{ keys[0].lang_type }}", "values | nosuchfilter");
        let error = render(
            &target,
            &schema(json!([key("NODE_ENV", "enum", json!(["a"]))])),
            "9.9.9",
        )
        .unwrap_err();
        assert!(error.to_string().contains("target fake"));
    }

    #[test]
    fn a_typo_in_a_template_fails_instead_of_rendering_a_blank() {
        let error = render(
            &target("{{ keys[0].nmae }}", "values | join('|')"),
            &schema(json!([key("PORT", "port", json!([]))])),
            "9.9.9",
        )
        .unwrap_err();
        assert!(matches!(error, Error::Render { .. }));
    }

    #[test]
    fn the_case_filters_agree_on_the_words() {
        let out = rendered(
            "{{ keys[0].name | pascal }} {{ keys[0].name | camel }} {{ keys[0].name | snake }}",
            json!([key("NEXT_PUBLIC_APP_URL", "string", json!([]))]),
        );
        assert_eq!(out, "NextPublicAppUrl nextPublicAppUrl next_public_app_url");
    }

    #[test]
    fn the_case_filters_split_humps_too() {
        let out = rendered(
            "{{ keys[0].name | snake }} {{ keys[0].name | pascal }}",
            json!([key("nextPublicAppUrl", "string", json!([]))]),
        );
        assert_eq!(out, "next_public_app_url NextPublicAppUrl");
    }

    #[test]
    fn quote_escapes_what_a_string_literal_cannot_hold() {
        let out = rendered(
            "{{ keys[0].name | quote }}",
            json!([key("A\"B", "string", json!([]))]),
        );
        assert_eq!(out, "\"A\\\"B\"");
    }

    #[test]
    fn json_writes_a_list_a_template_can_paste() {
        let out = rendered(
            "{{ keys[0].type.members | json }}",
            json!([key("NODE_ENV", "enum", json!(["a", "b"]))]),
        );
        assert_eq!(out, "[\"a\",\"b\"]");
    }

    #[test]
    fn a_target_missing_a_type_the_schema_uses_says_which_one() {
        let mut target = target("{{ keys[0].lang_type }}", "values | join('|')");
        target.types.remove("port");
        let error = render(
            &target,
            &schema(json!([key("PORT", "port", json!([]))])),
            "9.9.9",
        )
        .unwrap_err();
        assert_eq!(
            error,
            Error::NoTypeFor {
                target: "fake".into(),
                base: "port".into()
            }
        );
    }

    #[test]
    fn a_built_in_folder_is_a_folder_like_any_other() {
        for built_in in crate::BUILT_IN {
            parse(
                built_in.name,
                built_in.config,
                built_in.template,
                Source::BuiltIn,
                "built in",
            )
            .unwrap();
        }
    }
}
