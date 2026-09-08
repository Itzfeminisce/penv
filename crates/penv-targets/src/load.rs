use crate::error::Error;
use crate::target::{Source, Target, parse};

/// Reading a tree of folders, so the loader is a pure function over one.
pub trait Tree {
    fn read(&self, path: &str) -> Option<String>;
    /// Names of the directories directly under `path`.
    fn dirs(&self, path: &str) -> Vec<String>;
    fn exists(&self, path: &str) -> bool {
        self.read(path).is_some()
    }
}

/// Where targets are looked for. `repo` is the directory holding `.env.schema`.
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
    pub template: &'static str,
}

/// The folders shipped inside the binary. The layout on disk is the same one a
/// user target uses.
pub const BUILT_IN: &[BuiltIn] = &[
    BuiltIn {
        name: "ts",
        config: include_str!("../targets/ts/target.toml"),
        template: include_str!("../targets/ts/env.tmpl"),
    },
    BuiltIn {
        name: "py",
        config: include_str!("../targets/py/target.toml"),
        template: include_str!("../targets/py/env.tmpl"),
    },
];

fn folders(roots: &Roots, name: &str) -> Vec<(Source, String)> {
    let mut out = vec![(Source::Repo, format!("{}/.penv/targets/{name}", roots.repo))];
    if let Some(home) = &roots.home {
        out.push((Source::User, format!("{home}/.penv/targets/{name}")));
    }
    out
}

/// Repo folder, then home folder, then built in. The first found wins and says
/// where it came from.
pub fn load(tree: &dyn Tree, roots: &Roots, name: &str) -> Result<Target, Error> {
    let mut looked = Vec::new();
    for (source, dir) in folders(roots, name) {
        looked.push(dir.clone());
        let Some(config) = tree.read(&format!("{dir}/target.toml")) else {
            continue;
        };
        let template = tree
            .read(&format!("{dir}/env.tmpl"))
            .ok_or_else(|| Error::Malformed {
                dir: dir.clone(),
                message: "there is a target.toml but no env.tmpl".into(),
            })?;
        return parse(name, &config, &template, source, &dir);
    }

    looked.push("built in".into());
    match BUILT_IN.iter().find(|b| b.name == name) {
        Some(b) => parse(name, b.config, b.template, Source::BuiltIn, "built in"),
        None => Err(Error::NotFound {
            name: name.to_string(),
            looked,
        }),
    }
}

/// Every target that can be loaded, built in ones plus whatever the two `.penv`
/// folders add, each resolved through the same lookup order.
pub fn available(tree: &dyn Tree, roots: &Roots) -> Vec<Target> {
    let mut names: Vec<String> = BUILT_IN.iter().map(|b| b.name.to_string()).collect();
    let mut roots_dirs = vec![format!("{}/.penv/targets", roots.repo)];
    if let Some(home) = &roots.home {
        roots_dirs.push(format!("{home}/.penv/targets"));
    }
    for dir in roots_dirs {
        for name in tree.dirs(&dir) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names.sort();
    names
        .iter()
        .filter_map(|name| load(tree, roots, name).ok())
        .collect()
}

/// True when a file this target names is in the repository.
pub fn detected(tree: &dyn Tree, roots: &Roots, target: &Target) -> bool {
    target
        .detect
        .iter()
        .any(|file| tree.exists(&format!("{}/{file}", roots.repo)))
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

    const TYPES: &str = r#"
[types]
string = "S"
number = "N"
integer = "I"
boolean = "B"
url = "U"
email = "E"
port = "P"
enum = "values | join('|')"
"#;

    fn roots() -> Roots {
        Roots::new("/repo", Some("/home".into()))
    }

    fn custom(name: &str) -> String {
        format!("name = \"{name}\"\noutput = \"out.txt\"\n{TYPES}")
    }

    #[test]
    fn built_in_targets_load_with_no_tree_at_all() {
        let tree = Fake::default();
        let ts = load(&tree, &roots(), "ts").unwrap();
        assert_eq!(ts.source, Source::BuiltIn);
        assert_eq!(ts.output, "src/env.ts");
        let py = load(&tree, &roots(), "py").unwrap();
        assert_eq!(py.output, "penv_env.py");
    }

    #[test]
    fn the_repo_folder_beats_the_home_folder_and_the_built_in_one() {
        let tree = Fake::default()
            .with("/repo/.penv/targets/ts/target.toml", &custom("ts"))
            .with("/repo/.penv/targets/ts/env.tmpl", "repo")
            .with("/home/.penv/targets/ts/target.toml", &custom("ts"))
            .with("/home/.penv/targets/ts/env.tmpl", "home");
        let target = load(&tree, &roots(), "ts").unwrap();
        assert_eq!(target.source, Source::Repo);
        assert_eq!(target.template, "repo");
        assert_eq!(target.dir, "/repo/.penv/targets/ts");
    }

    #[test]
    fn the_home_folder_beats_the_built_in_one() {
        let tree = Fake::default()
            .with("/home/.penv/targets/py/target.toml", &custom("py"))
            .with("/home/.penv/targets/py/env.tmpl", "home");
        let target = load(&tree, &roots(), "py").unwrap();
        assert_eq!(target.source, Source::User);
        assert_eq!(target.output, "out.txt");
    }

    #[test]
    fn an_unknown_name_says_where_it_looked() {
        let error = load(&Fake::default(), &roots(), "go").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("/repo/.penv/targets/go"), "{message}");
        assert!(message.contains("/home/.penv/targets/go"), "{message}");
        assert!(message.contains("built in"), "{message}");
    }

    #[test]
    fn a_target_toml_with_no_template_beside_it_is_a_broken_folder() {
        let tree = Fake::default().with("/repo/.penv/targets/ts/target.toml", &custom("ts"));
        let error = load(&tree, &roots(), "ts").unwrap_err();
        assert!(error.to_string().contains("no env.tmpl"));
    }

    #[test]
    fn available_lists_the_built_in_pair_plus_whatever_the_folders_add() {
        let tree = Fake::default()
            .with("/repo/.penv/targets/go/target.toml", &custom("go"))
            .with("/repo/.penv/targets/go/env.tmpl", "x");
        let found = available(&tree, &roots());
        let names: Vec<&str> = found.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["go", "py", "ts"]);
    }

    #[test]
    fn a_target_is_detected_by_its_own_files_in_the_repo() {
        let tree = Fake::default().with("/repo/package.json", "{}");
        let r = roots();
        let ts = load(&tree, &r, "ts").unwrap();
        let py = load(&tree, &r, "py").unwrap();
        assert!(detected(&tree, &r, &ts));
        assert!(!detected(&tree, &r, &py));
    }
}
