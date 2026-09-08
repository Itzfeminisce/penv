use crate::error::Error;
use crate::guard::{Guard, parse};

/// Reading a tree of folders, so the loader is a pure function over one.
pub trait Tree {
    fn read(&self, path: &str) -> Option<String>;
    /// Names of the directories directly under `path`.
    fn dirs(&self, path: &str) -> Vec<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    pub repo: String,
    pub home: Option<String>,
}

impl Roots {
    pub fn new(repo: impl Into<String>, home: Option<String>) -> Roots {
        Roots {
            repo: repo.into(),
            home,
        }
    }
}

pub struct BuiltIn {
    pub name: &'static str,
    pub config: &'static str,
    pub templates: &'static [(&'static str, &'static str)],
}

macro_rules! built_in {
    ($name:literal, [$($template:literal),* $(,)?]) => {
        BuiltIn {
            name: $name,
            config: include_str!(concat!("../guards/", $name, "/guard.toml")),
            templates: &[
                $(($template, include_str!(concat!("../guards/", $name, "/", $template)))),*
            ],
        }
    };
}

/// The harnesses penv knows, in the order the design ranks them.
pub const BUILT_IN: &[BuiltIn] = &[
    built_in!(
        "claude-code",
        ["settings.json.tmpl", "user-settings.json.tmpl"]
    ),
    built_in!("codex", ["config.toml.tmpl"]),
    built_in!("cursor", ["cli.json.tmpl", "hooks.json.tmpl"]),
    built_in!("amp", ["settings.json.tmpl"]),
    built_in!("copilot", ["permissions-config.json.tmpl"]),
    built_in!("gemini", ["settings.json.tmpl"]),
    built_in!("cline", ["PreToolUse.tmpl"]),
    built_in!("windsurf", ["hooks.json.tmpl"]),
];

fn folders(roots: &Roots, name: &str) -> Vec<String> {
    let mut out = vec![format!("{}/.penv/guards/{name}", roots.repo)];
    if let Some(home) = &roots.home {
        out.push(format!("{home}/.penv/guards/{name}"));
    }
    out
}

/// Repo folder, then home folder, then built in. The first found wins.
pub fn load(tree: &dyn Tree, roots: &Roots, name: &str) -> Result<Guard, Error> {
    let mut looked = Vec::new();
    for dir in folders(roots, name) {
        looked.push(dir.clone());
        let Some(config) = tree.read(&format!("{dir}/guard.toml")) else {
            continue;
        };
        return parse(
            name,
            &config,
            &|template| tree.read(&format!("{dir}/{template}")),
            &dir,
        );
    }

    looked.push("built in".into());
    match BUILT_IN.iter().find(|b| b.name == name) {
        Some(b) => parse(
            name,
            b.config,
            &|template| {
                b.templates
                    .iter()
                    .find(|(n, _)| *n == template)
                    .map(|(_, body)| (*body).to_string())
            },
            "built in",
        ),
        None => Err(Error::NotFound {
            name: name.to_string(),
            looked,
        }),
    }
}

/// Every guard that can be loaded: the ranked built-in list first, then whatever
/// the two `.penv` folders add.
pub fn available(tree: &dyn Tree, roots: &Roots) -> Vec<Guard> {
    let mut names: Vec<String> = BUILT_IN.iter().map(|b| b.name.to_string()).collect();
    let mut dirs = vec![format!("{}/.penv/guards", roots.repo)];
    if let Some(home) = &roots.home {
        dirs.push(format!("{home}/.penv/guards"));
    }
    for dir in dirs {
        for name in tree.dirs(&dir) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
        .iter()
        .filter_map(|name| load(tree, roots, name).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::{Format, Merge, Scope};
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Fake(BTreeMap<String, String>);

    impl Fake {
        fn with(mut self, path: &str, contents: &str) -> Fake {
            self.0.insert(path.to_string(), contents.to_string());
            self
        }
    }

    impl Tree for Fake {
        fn read(&self, path: &str) -> Option<String> {
            self.0.get(path).cloned()
        }
        fn dirs(&self, path: &str) -> Vec<String> {
            let prefix = format!("{path}/");
            let mut out: Vec<String> = self
                .0
                .keys()
                .filter_map(|p| p.strip_prefix(&prefix))
                .filter_map(|rest| rest.split('/').next())
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect();
            out.dedup();
            out
        }
    }

    const CONFIG: &str = r#"
name = "claude-code"
detect = [".claude"]

[[write]]
path = ".claude/settings.json"
format = "json"
merge = "deny-union"
template = "settings.json.tmpl"
"#;

    fn roots() -> Roots {
        Roots::new("/repo", Some("/home".into()))
    }

    #[test]
    fn every_built_in_guard_is_a_folder_that_parses() {
        let found = available(&Fake::default(), &roots());
        let names: Vec<&str> = found.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "claude-code",
                "codex",
                "cursor",
                "amp",
                "copilot",
                "gemini",
                "cline",
                "windsurf"
            ]
        );
    }

    #[test]
    fn the_repo_folder_beats_the_built_in_one() {
        let tree = Fake::default()
            .with("/repo/.penv/guards/claude-code/guard.toml", CONFIG)
            .with("/repo/.penv/guards/claude-code/settings.json.tmpl", "{}");
        let guard = load(&tree, &roots(), "claude-code").unwrap();
        assert_eq!(guard.dir, "/repo/.penv/guards/claude-code");
        assert_eq!(guard.writes.len(), 1);
    }

    #[test]
    fn an_unknown_harness_says_where_it_looked() {
        let error = load(&Fake::default(), &roots(), "nano").unwrap_err();
        assert!(error.to_string().contains("/home/.penv/guards/nano"));
    }

    #[test]
    fn claude_code_writes_the_project_file_and_hands_back_the_user_block() {
        let guard = load(&Fake::default(), &roots(), "claude-code").unwrap();
        let project: Vec<&str> = guard.project_writes().map(|w| w.path.as_str()).collect();
        assert_eq!(project, [".claude/settings.json"]);
        assert_eq!(guard.user_writes().count(), 1);
        assert_eq!(guard.writes[0].format, Format::Json);
        assert_eq!(guard.writes[0].merge, Merge::DenyUnion);
        assert_eq!(guard.writes[1].scope, Scope::User);
    }

    #[test]
    fn codex_merges_toml_and_cline_appends_a_script_line() {
        let codex = load(&Fake::default(), &roots(), "codex").unwrap();
        assert_eq!(codex.writes[0].format, Format::Toml);
        assert_eq!(codex.writes[0].merge, Merge::AppendUnique);
        let cline = load(&Fake::default(), &roots(), "cline").unwrap();
        assert_eq!(cline.writes[0].format, Format::Text);
        assert_eq!(cline.writes[0].path, ".clinerules/hooks/PreToolUse");
    }
}
