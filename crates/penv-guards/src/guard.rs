use serde::Deserialize;

use crate::error::Error;

/// The files that hold values. `.env.schema` is deliberately absent: it holds
/// none, and an agent that cannot read it cannot help with the keys.
pub const ENV_FILES: [&str; 10] = [
    ".env",
    ".env.local",
    ".env.development",
    ".env.development.local",
    ".env.staging",
    ".env.staging.local",
    ".env.production",
    ".env.production.local",
    ".env.test",
    ".env.test.local",
];

/// Array keys a merge unions instead of leaving alone.
const DEFAULT_UNION: [&str; 5] = ["deny", "denyRead", "files", "envVars", "hooks"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Project,
    User,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Project => "project",
            Scope::User => "user",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Json,
    Toml,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Merge {
    /// Deep merge; named arrays are unioned, nothing existing is removed.
    DenyUnion,
    /// Append what is not already there, line by line or entry by entry.
    AppendUnique,
    /// Add absent keys only; an existing setting is the user's, not ours.
    ReplaceIfAbsent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    pub path: String,
    pub format: Format,
    pub merge: Merge,
    pub scope: Scope,
    pub union: Vec<String>,
    pub template: String,
    /// The template body, read from the folder beside `guard.toml`.
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guard {
    pub name: String,
    pub description: Option<String>,
    pub scope: Scope,
    /// Paths relative to the repository, or `~`-prefixed for the home directory.
    pub detect: Vec<String>,
    /// Executables whose presence on PATH also means installed.
    pub exe: Vec<String>,
    pub writes: Vec<Write>,
    pub dir: String,
}

impl Guard {
    /// The writes a `guard` run puts on disk.
    pub fn project_writes(&self) -> impl Iterator<Item = &Write> {
        self.writes.iter().filter(|w| w.scope == Scope::Project)
    }

    /// The blocks a person has to paste into their own settings.
    pub fn user_writes(&self) -> impl Iterator<Item = &Write> {
        self.writes.iter().filter(|w| w.scope == Scope::User)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default = "project")]
    scope: Scope,
    #[serde(default)]
    detect: Vec<String>,
    #[serde(default)]
    exe: Vec<String>,
    #[serde(default, rename = "write")]
    writes: Vec<WriteFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteFile {
    path: String,
    format: Format,
    merge: Merge,
    #[serde(default)]
    scope: Option<Scope>,
    #[serde(default)]
    union: Option<Vec<String>>,
    template: String,
}

fn project() -> Scope {
    Scope::Project
}

/// Read one guard folder whose files have already been fetched into strings.
pub fn parse(
    name: &str,
    config: &str,
    templates: &dyn Fn(&str) -> Option<String>,
    dir: &str,
) -> Result<Guard, Error> {
    let file: File = toml::from_str(config).map_err(|e| Error::Malformed {
        dir: dir.to_string(),
        message: e.message().to_string(),
    })?;
    if file.name != name {
        return Err(Error::Malformed {
            dir: dir.to_string(),
            message: format!("guard.toml names {}, the folder names {name}", file.name),
        });
    }
    if file.writes.is_empty() {
        return Err(Error::Malformed {
            dir: dir.to_string(),
            message: "a guard with no [[write]] entry writes nothing".into(),
        });
    }

    let writes = file
        .writes
        .into_iter()
        .map(|w| {
            let body = templates(&w.template).ok_or_else(|| Error::Malformed {
                dir: dir.to_string(),
                message: format!("{} is missing", w.template),
            })?;
            Ok(Write {
                path: w.path,
                format: w.format,
                merge: w.merge,
                scope: w.scope.unwrap_or(file.scope),
                union: w
                    .union
                    .unwrap_or_else(|| DEFAULT_UNION.iter().map(|s| s.to_string()).collect()),
                template: w.template,
                body,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;

    Ok(Guard {
        name: file.name,
        description: file.description,
        scope: file.scope,
        detect: file.detect,
        exe: file.exe,
        writes,
        dir: dir.to_string(),
    })
}

/// What a harness looks like when it is installed.
pub trait Probe {
    /// `path` is a detect entry exactly as the guard wrote it.
    fn exists(&self, path: &str) -> bool;
    fn on_path(&self, exe: &str) -> bool;
}

pub fn is_installed(guard: &Guard, probe: &dyn Probe) -> bool {
    guard.detect.iter().any(|p| probe.exists(p)) || guard.exe.iter().any(|e| probe.on_path(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn templates(name: &str) -> Option<String> {
        Some(format!("body of {name}"))
    }

    const CONFIG: &str = r#"
name = "acme"
description = "an example"
detect = [".acme", "~/.acme"]
exe = ["acme"]

[[write]]
path = ".acme/settings.json"
format = "json"
merge = "deny-union"
template = "settings.json.tmpl"

[[write]]
path = "~/.acme/settings.json"
scope = "user"
format = "json"
merge = "deny-union"
union = ["envVars"]
template = "user.json.tmpl"
"#;

    fn guard() -> Guard {
        parse("acme", CONFIG, &templates, "guards/acme").unwrap()
    }

    #[test]
    fn a_folder_of_toml_and_templates_is_a_guard() {
        let guard = guard();
        assert_eq!(guard.detect, [".acme", "~/.acme"]);
        assert_eq!(guard.exe, ["acme"]);
        assert_eq!(guard.writes[0].body, "body of settings.json.tmpl");
        assert_eq!(guard.writes[0].format, Format::Json);
        assert_eq!(guard.writes[0].merge, Merge::DenyUnion);
    }

    #[test]
    fn a_write_inherits_the_guard_scope_and_can_override_it() {
        let guard = guard();
        assert_eq!(guard.writes[0].scope, Scope::Project);
        assert_eq!(guard.writes[1].scope, Scope::User);
        assert_eq!(guard.project_writes().count(), 1);
        assert_eq!(guard.user_writes().count(), 1);
    }

    #[test]
    fn the_union_list_defaults_to_the_keys_that_hold_rules() {
        let guard = guard();
        assert!(guard.writes[0].union.iter().any(|k| k == "denyRead"));
        assert_eq!(guard.writes[1].union, ["envVars"]);
    }

    #[test]
    fn a_named_template_that_is_not_in_the_folder_is_a_broken_guard() {
        let error = parse("acme", CONFIG, &|_| None, "guards/acme").unwrap_err();
        assert!(error.to_string().contains("is missing"));
    }

    #[test]
    fn a_guard_that_writes_nothing_is_refused() {
        let error = parse("acme", "name = \"acme\"\n", &templates, "guards/acme").unwrap_err();
        assert!(error.to_string().contains("writes nothing"));
    }

    struct Fake<'a>(&'a [&'a str], &'a [&'a str]);

    impl Probe for Fake<'_> {
        fn exists(&self, path: &str) -> bool {
            self.0.contains(&path)
        }
        fn on_path(&self, exe: &str) -> bool {
            self.1.contains(&exe)
        }
    }

    #[test]
    fn a_harness_is_installed_when_a_folder_or_the_binary_is_there() {
        let guard = guard();
        assert!(is_installed(&guard, &Fake(&["~/.acme"], &[])));
        assert!(is_installed(&guard, &Fake(&[], &["acme"])));
        assert!(!is_installed(&guard, &Fake(&["~/.other"], &["other"])));
    }
}
