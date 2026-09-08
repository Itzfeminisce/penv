//! The folder lookup and the template helpers that targets and guards share.
//!
//! A drop-in part is a directory under `.penv/<kind>/<name>/` in the repository,
//! under `~/.penv/<kind>/<name>/`, or shipped inside the binary. The order is the
//! same for every kind, so only the files inside a folder differ.

use minijinja::value::Value as Jinja;
use minijinja::{AutoEscape, Environment, UndefinedBehavior};

/// Reading a tree of folders, so the loader is a pure function over one.
pub trait Tree {
    fn read(&self, path: &str) -> Option<String>;
    /// Names of the directories directly under `path`.
    fn dirs(&self, path: &str) -> Vec<String>;
    fn exists(&self, path: &str) -> bool {
        self.read(path).is_some()
    }
}

/// Where folders are looked for. `repo` is the directory holding `.env.schema`.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Repo,
    User,
    BuiltIn,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Repo => "repo",
            Source::User => "user",
            Source::BuiltIn => "built in",
        }
    }
}

/// A folder compiled into the binary, as the files it holds by name.
pub struct BuiltIn {
    pub name: &'static str,
    pub files: &'static [(&'static str, &'static str)],
}

impl BuiltIn {
    pub fn file(&self, name: &str) -> Option<&'static str> {
        self.files
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, body)| *body)
    }
}

/// One folder that was found, with its files still unread.
#[derive(Debug)]
pub struct Found<'a> {
    pub source: Source,
    pub dir: String,
    read: Reader<'a>,
}

type ReadFile<'a> = Box<dyn Fn(&str) -> Option<String> + 'a>;

struct Reader<'a>(ReadFile<'a>);

impl std::fmt::Debug for Reader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<files>")
    }
}

impl Found<'_> {
    pub fn file(&self, name: &str) -> Option<String> {
        (self.read.0)(name)
    }
}

fn dirs(roots: &Roots, kind: &str, name: &str) -> Vec<(Source, String)> {
    let mut out = vec![(Source::Repo, format!("{}/.penv/{kind}/{name}", roots.repo))];
    if let Some(home) = &roots.home {
        out.push((Source::User, format!("{home}/.penv/{kind}/{name}")));
    }
    out
}

/// Repo folder, then home folder, then built in. The first one holding `entry`
/// wins; the error is the list of places that were looked in.
pub fn find<'a>(
    tree: &'a dyn Tree,
    roots: &Roots,
    kind: &str,
    entry: &str,
    built_in: &'static [BuiltIn],
    name: &str,
) -> Result<Found<'a>, Vec<String>> {
    let mut looked = Vec::new();
    for (source, dir) in dirs(roots, kind, name) {
        looked.push(dir.clone());
        if tree.read(&format!("{dir}/{entry}")).is_none() {
            continue;
        }
        let prefix = dir.clone();
        return Ok(Found {
            source,
            dir,
            read: Reader(Box::new(move |file| tree.read(&format!("{prefix}/{file}")))),
        });
    }

    looked.push("built in".into());
    match built_in.iter().find(|b| b.name == name) {
        Some(b) => Ok(Found {
            source: Source::BuiltIn,
            dir: "built in".into(),
            read: Reader(Box::new(move |file| b.file(file).map(str::to_string))),
        }),
        None => Err(looked),
    }
}

/// The built-in names in their own order, then whatever the two `.penv` folders
/// add.
pub fn names(
    tree: &dyn Tree,
    roots: &Roots,
    kind: &str,
    built_in: &'static [BuiltIn],
) -> Vec<String> {
    let mut names: Vec<String> = built_in.iter().map(|b| b.name.to_string()).collect();
    let mut roots_dirs = vec![format!("{}/.penv/{kind}", roots.repo)];
    if let Some(home) = &roots.home {
        roots_dirs.push(format!("{home}/.penv/{kind}"));
    }
    for dir in roots_dirs {
        for name in tree.dirs(&dir) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// The template engine every folder renders through: a typo is an error, and
/// nothing is HTML.
pub fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env.add_filter("quote", quote);
    env.add_filter("json", to_json);
    env
}

pub fn quote(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\""))
}

pub fn to_json(value: Jinja) -> Result<String, minijinja::Error> {
    serde_json::to_string(&value).map_err(|e| {
        minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!("value cannot be written as JSON: {e}"),
        )
    })
}

/// A minijinja failure with the detail line it hides behind `detail()`.
pub fn describe(error: &minijinja::Error) -> String {
    match error.detail() {
        Some(detail) => format!("{error}: {detail}"),
        None => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    const BUILT_IN: &[BuiltIn] = &[BuiltIn {
        name: "ts",
        files: &[("target.toml", "built in config"), ("env.tmpl", "built in")],
    }];

    fn roots() -> Roots {
        Roots::new("/repo", Some("/home".into()))
    }

    #[test]
    fn the_repo_folder_beats_the_home_folder_and_the_built_in_one() {
        let tree = Fake::default()
            .with("/repo/.penv/targets/ts/target.toml", "repo config")
            .with("/repo/.penv/targets/ts/env.tmpl", "repo")
            .with("/home/.penv/targets/ts/target.toml", "home config");
        let found = find(&tree, &roots(), "targets", "target.toml", BUILT_IN, "ts").unwrap();
        assert_eq!(found.source, Source::Repo);
        assert_eq!(found.file("env.tmpl").as_deref(), Some("repo"));
    }

    #[test]
    fn the_built_in_folder_is_the_last_place_looked() {
        let tree = Fake::default();
        let found = find(&tree, &roots(), "targets", "target.toml", BUILT_IN, "ts").unwrap();
        assert_eq!(found.source, Source::BuiltIn);
        assert_eq!(found.file("env.tmpl").as_deref(), Some("built in"));
    }

    #[test]
    fn an_unknown_name_reports_every_place_it_looked() {
        let tree = Fake::default();
        let looked = find(&tree, &roots(), "guards", "guard.toml", BUILT_IN, "nano").unwrap_err();
        assert_eq!(
            looked,
            [
                "/repo/.penv/guards/nano",
                "/home/.penv/guards/nano",
                "built in"
            ]
        );
    }

    #[test]
    fn the_names_are_the_built_in_ones_plus_the_folders() {
        let tree = Fake::default().with("/home/.penv/targets/go/target.toml", "x");
        assert_eq!(names(&tree, &roots(), "targets", BUILT_IN), ["ts", "go"]);
    }
}
