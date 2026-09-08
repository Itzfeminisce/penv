use minijinja::value::Value as Jinja;
use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use serde_json::{Value, json};

use crate::error::Error;
use crate::guard::{ENV_FILES, Guard, Write};

/// A guard template sees the schema JSON, `penv.version` and `env_files`. Key
/// names are all a harness needs; values never enter the context.
pub fn render(
    guard: &Guard,
    write: &Write,
    schema: &Value,
    penv_version: &str,
) -> Result<String, Error> {
    let mut context = schema.clone();
    context["penv"] = json!({ "version": penv_version });
    context["env_files"] = json!(ENV_FILES);

    environment()
        .render_str(&write.body, Jinja::from_serialize(&context))
        .map_err(|e| Error::Render {
            guard: guard.name.clone(),
            message: match e.detail() {
                Some(detail) => format!("{}: {e}: {detail}", write.template),
                None => format!("{}: {e}", write.template),
            },
        })
}

fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env.add_filter("quote", quote);
    env.add_filter("json", to_json);
    env
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
    use crate::load::{Roots, Tree, load};

    struct Empty;

    impl Tree for Empty {
        fn read(&self, _path: &str) -> Option<String> {
            None
        }
        fn dirs(&self, _path: &str) -> Vec<String> {
            Vec::new()
        }
    }

    fn schema() -> Value {
        json!({
            "schemaVersion": 1,
            "org": "acme",
            "project": "api-gateway",
            "defaultSensitive": true,
            "defaultRequired": true,
            "keys": [
                key("STRIPE_SECRET_KEY", true),
                key("NEXT_PUBLIC_APP_URL", false),
                key("DATABASE_URL", true),
            ],
        })
    }

    fn key(name: &str, sensitive: bool) -> Value {
        json!({
            "name": name,
            "description": null,
            "type": { "name": "string", "raw": "string", "members": [], "constraints": {} },
            "required": true,
            "sensitive": sensitive,
            "default": "not-a-real-value",
            "example": null,
            "docs": null,
            "since": null,
            "deprecated": null,
            "rotate": null,
            "dynamic": null,
        })
    }

    fn guard(name: &str) -> Guard {
        load(&Empty, &Roots::new("/repo", None), name).unwrap()
    }

    fn rendered(name: &str, index: usize) -> String {
        let guard = guard(name);
        render(&guard, &guard.writes[index], &schema(), "9.9.9").unwrap()
    }

    #[test]
    fn every_built_in_guard_renders_the_format_it_declares() {
        for built_in in crate::BUILT_IN {
            let guard = guard(built_in.name);
            for (index, write) in guard.writes.iter().enumerate() {
                let out = rendered(built_in.name, index);
                match write.format {
                    crate::Format::Json => {
                        serde_json::from_str::<Value>(&out).unwrap_or_else(|e| {
                            panic!("{}/{} is not JSON: {e}", built_in.name, write.template)
                        });
                    }
                    crate::Format::Toml => {
                        toml::from_str::<toml::Value>(&out).unwrap_or_else(|e| {
                            panic!("{}/{} is not TOML: {e}", built_in.name, write.template)
                        });
                    }
                    crate::Format::Text => assert!(!out.is_empty()),
                }
            }
        }
    }

    #[test]
    fn the_deny_list_names_the_env_files_and_leaves_the_schema_readable() {
        let out = rendered("claude-code", 0);
        assert!(out.contains("Read(./.env)"));
        assert!(out.contains("Read(./.env.production)"));
        assert!(!out.contains(".env.schema"));
        assert!(!out.contains(".env.*"), "a glob would catch .env.schema");
    }

    #[test]
    fn the_claude_code_hook_runs_the_binary_itself() {
        let out: Value = serde_json::from_str(&rendered("claude-code", 0)).unwrap();
        let hook = &out["hooks"]["PreToolUse"][0];
        assert_eq!(hook["matcher"], "Bash");
        assert_eq!(hook["hooks"][0]["command"], "penv hook claude-code");
    }

    #[test]
    fn the_user_block_masks_every_sensitive_key_and_no_other() {
        let out: Value = serde_json::from_str(&rendered("claude-code", 1)).unwrap();
        let vars = out["sandbox"]["credentials"]["envVars"].as_array().unwrap();
        let names: Vec<&str> = vars.iter().map(|v| v["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["STRIPE_SECRET_KEY", "DATABASE_URL"]);
        assert!(vars.iter().all(|v| v["mode"] == "mask"));
        assert_eq!(out["sandbox"]["credentials"]["injectHosts"], json!([]));
    }

    #[test]
    fn no_rendered_guard_carries_a_value() {
        for built_in in crate::BUILT_IN {
            let guard = guard(built_in.name);
            for index in 0..guard.writes.len() {
                let out = rendered(built_in.name, index);
                assert!(
                    !out.contains("not-a-real-value"),
                    "{} wrote a value into {}",
                    built_in.name,
                    guard.writes[index].path
                );
            }
        }
    }
}
