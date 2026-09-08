use serde_json::Value;

use crate::error::Error;
use crate::guard::{Format, Merge, Write};

/// What a write would leave on disk, and whether that differs from what is
/// there now. Applying the same fragment twice changes nothing the second time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub content: String,
    pub changed: bool,
}

/// Fold a rendered fragment into whatever is already at the path. Nothing an
/// existing file says is ever removed or overwritten.
pub fn apply(write: &Write, existing: Option<&str>, fragment: &str) -> Result<Outcome, Error> {
    match write.format {
        Format::Json => json(write, existing, fragment),
        Format::Toml => toml_format(write, existing, fragment),
        Format::Text => Ok(text(existing.unwrap_or_default(), fragment)),
    }
}

fn json(write: &Write, existing: Option<&str>, fragment: &str) -> Result<Outcome, Error> {
    let add: Value = serde_json::from_str(fragment).map_err(|e| Error::Unreadable {
        path: write.template.clone(),
        message: format!("the template did not render JSON: {e}"),
    })?;
    let Some(source) = existing else {
        return Ok(Outcome {
            content: write_json(&add),
            changed: true,
        });
    };
    let mut base: Value = serde_json::from_str(source).map_err(|e| Error::Unreadable {
        path: write.path.clone(),
        message: format!("the file on disk is not JSON: {e}"),
    })?;
    let before = base.clone();
    merge_json(&mut base, &add, "", &write.union, write.merge);
    Ok(if base == before {
        Outcome {
            content: source.to_string(),
            changed: false,
        }
    } else {
        Outcome {
            content: write_json(&base),
            changed: true,
        }
    })
}

fn write_json(value: &Value) -> String {
    let mut out = serde_json::to_string_pretty(value).unwrap_or_default();
    out.push('\n');
    out
}

fn merge_json(base: &mut Value, add: &Value, key: &str, union: &[String], mode: Merge) {
    match (base, add) {
        (Value::Object(base), Value::Object(add)) => {
            for (name, value) in add {
                match base.get_mut(name) {
                    Some(slot) => merge_json(slot, value, name, union, mode),
                    None => {
                        base.insert(name.clone(), value.clone());
                    }
                }
            }
        }
        (Value::Array(base), Value::Array(add)) if unites(key, union, mode) => {
            for item in add {
                if !base.contains(item) {
                    base.push(item.clone());
                }
            }
        }
        _ => {}
    }
}

fn unites(key: &str, union: &[String], mode: Merge) -> bool {
    match mode {
        Merge::DenyUnion => union.iter().any(|k| k == key),
        Merge::AppendUnique => true,
        Merge::ReplaceIfAbsent => false,
    }
}

fn toml_format(write: &Write, existing: Option<&str>, fragment: &str) -> Result<Outcome, Error> {
    let add: toml::Value = toml::from_str(fragment).map_err(|e| Error::Unreadable {
        path: write.template.clone(),
        message: format!("the template did not render TOML: {}", e.message()),
    })?;
    let Some(source) = existing else {
        return Ok(Outcome {
            content: write_toml(&add)?,
            changed: true,
        });
    };
    let mut base: toml::Value = toml::from_str(source).map_err(|e| Error::Unreadable {
        path: write.path.clone(),
        message: format!("the file on disk is not TOML: {}", e.message()),
    })?;
    let before = base.clone();
    merge_toml(&mut base, &add);
    Ok(if base == before {
        Outcome {
            content: source.to_string(),
            changed: false,
        }
    } else {
        Outcome {
            content: write_toml(&base)?,
            changed: true,
        }
    })
}

fn write_toml(value: &toml::Value) -> Result<String, Error> {
    toml::to_string(value).map_err(|e| Error::Unreadable {
        path: "the merged file".into(),
        message: e.to_string(),
    })
}

fn merge_toml(base: &mut toml::Value, add: &toml::Value) {
    match (base, add) {
        (toml::Value::Table(base), toml::Value::Table(add)) => {
            for (name, value) in add {
                match base.get_mut(name) {
                    Some(slot) => merge_toml(slot, value),
                    None => {
                        base.insert(name.clone(), value.clone());
                    }
                }
            }
        }
        (toml::Value::Array(base), toml::Value::Array(add)) => {
            for item in add {
                if !base.contains(item) {
                    base.push(item.clone());
                }
            }
        }
        _ => {}
    }
}

fn text(existing: &str, fragment: &str) -> Outcome {
    let mut lines: Vec<&str> = existing.lines().collect();
    let mut changed = false;
    for line in fragment.lines() {
        if !lines.contains(&line) {
            lines.push(line);
            changed = true;
        }
    }
    if !changed {
        return Outcome {
            content: existing.to_string(),
            changed: false,
        };
    }
    let mut content = lines.join("\n");
    content.push('\n');
    Outcome {
        content,
        changed: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::Scope;

    fn write(format: Format, merge: Merge) -> Write {
        Write {
            path: "settings".into(),
            format,
            merge,
            scope: Scope::Project,
            union: ["deny", "denyRead", "hooks"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            executable: false,
            template: "settings.tmpl".into(),
            body: String::new(),
        }
    }

    fn twice(write: &Write, existing: Option<&str>, fragment: &str) -> Outcome {
        let once = apply(write, existing, fragment).unwrap();
        let again = apply(write, Some(&once.content), fragment).unwrap();
        assert!(!again.changed, "the second write changed the file");
        assert_eq!(once.content, again.content);
        once
    }

    #[test]
    fn a_first_json_write_is_the_fragment() {
        let out = twice(
            &write(Format::Json, Merge::DenyUnion),
            None,
            r#"{"permissions":{"deny":["Read(.env)"]}}"#,
        );
        assert!(out.changed);
        assert!(out.content.ends_with("\n"));
        assert!(out.content.contains("Read(.env)"));
    }

    #[test]
    fn a_named_array_gains_entries_and_loses_none() {
        let out = twice(
            &write(Format::Json, Merge::DenyUnion),
            Some(r#"{"permissions":{"deny":["Read(secrets.json)"]}}"#),
            r#"{"permissions":{"deny":["Read(.env)"]}}"#,
        );
        let merged: Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(
            merged["permissions"]["deny"],
            serde_json::json!(["Read(secrets.json)", "Read(.env)"])
        );
    }

    #[test]
    fn an_existing_setting_is_never_overwritten() {
        let out = apply(
            &write(Format::Json, Merge::DenyUnion),
            Some(r#"{"sandbox":{"enabled":false}}"#),
            r#"{"sandbox":{"enabled":true,"filesystem":{"denyRead":[".env"]}}}"#,
        )
        .unwrap();
        let merged: Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(merged["sandbox"]["enabled"], false);
        assert_eq!(
            merged["sandbox"]["filesystem"]["denyRead"],
            serde_json::json!([".env"])
        );
    }

    #[test]
    fn an_array_nobody_named_is_left_where_it_is() {
        let out = apply(
            &write(Format::Json, Merge::DenyUnion),
            Some(r#"{"allow":["Bash"]}"#),
            r#"{"allow":["Read"]}"#,
        )
        .unwrap();
        assert!(!out.changed);
    }

    #[test]
    fn replace_if_absent_only_fills_in_what_is_missing() {
        let out = apply(
            &write(Format::Json, Merge::ReplaceIfAbsent),
            Some(r#"{"amp.guardedFiles.allowlist":["notes.md"]}"#),
            r#"{"amp.guardedFiles.allowlist":[]}"#,
        )
        .unwrap();
        assert!(!out.changed);
    }

    #[test]
    fn a_json_file_that_is_not_json_says_so_instead_of_clobbering_it() {
        let error = apply(
            &write(Format::Json, Merge::DenyUnion),
            Some("// comments are not JSON\n{}"),
            "{}",
        )
        .unwrap_err();
        assert!(error.to_string().contains("not JSON"));
    }

    #[test]
    fn toml_tables_merge_and_arrays_append() {
        let out = twice(
            &write(Format::Toml, Merge::AppendUnique),
            Some("[sandbox_workspace_write]\ndeny_read = [\"**/secrets\"]\n"),
            "[sandbox_workspace_write]\ndeny_read = [\"**/.env\"]\n\n[shell_environment_policy]\ninherit = \"core\"\n",
        );
        let merged: toml::Value = toml::from_str(&out.content).unwrap();
        assert_eq!(
            merged["sandbox_workspace_write"]["deny_read"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            merged["shell_environment_policy"]["inherit"].as_str(),
            Some("core")
        );
    }

    #[test]
    fn text_appends_the_lines_that_are_not_there_yet() {
        let out = twice(
            &write(Format::Text, Merge::AppendUnique),
            Some("#!/bin/sh\n"),
            "#!/bin/sh\nexec penv hook cline\n",
        );
        assert_eq!(out.content, "#!/bin/sh\nexec penv hook cline\n");
    }
}
