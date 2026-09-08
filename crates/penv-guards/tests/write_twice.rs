//! The one test in this wave that touches a real filesystem: every built-in
//! guard, written into a temp tree twice, has to leave the same bytes.

use std::path::{Path, PathBuf};

use penv_guards::{BUILT_IN, Roots, Scope, Tree, apply, load, render};
use serde_json::Value;

const FIXTURE: &str = include_str!("../../penv-targets/tests/fixture.env.schema");

struct Empty;

impl Tree for Empty {
    fn read(&self, _path: &str) -> Option<String> {
        None
    }
    fn dirs(&self, _path: &str) -> Vec<String> {
        Vec::new()
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("penv-guards-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_all(root: &Path, schema: &Value) -> Vec<(PathBuf, String)> {
    let mut written = Vec::new();
    for built_in in BUILT_IN {
        let guard = load(&Empty, &Roots::new("/repo", None), built_in.name).unwrap();
        for entry in &guard.writes {
            if entry.scope != Scope::Project {
                continue;
            }
            let path = root.join(&entry.path);
            let existing = std::fs::read_to_string(&path).ok();
            let fragment = render(&guard, entry, schema, "1.0.0-test").unwrap();
            let outcome = apply(entry, existing.as_deref(), &fragment).unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &outcome.content).unwrap();
            written.push((path, outcome.content));
        }
    }
    written
}

#[test]
fn writing_every_guard_twice_leaves_the_same_bytes() {
    let schema = penv_schema::parse(FIXTURE).unwrap().to_json();
    let temp = TempDir::new("twice");

    let first = write_all(&temp.0, &schema);
    assert_eq!(
        first.len(),
        9,
        "every project write of every built-in guard"
    );
    let second = write_all(&temp.0, &schema);
    assert_eq!(first, second);

    let claude = std::fs::read_to_string(temp.0.join(".claude/settings.json")).unwrap();
    let parsed: Value = serde_json::from_str(&claude).unwrap();
    assert_eq!(parsed["permissions"]["deny"][0], "Read(./.env)");
    assert!(!claude.contains(".env.schema"));
}

#[test]
fn a_rule_a_person_already_wrote_survives_the_merge() {
    let schema = penv_schema::parse(FIXTURE).unwrap().to_json();
    let temp = TempDir::new("survives");
    let settings = temp.0.join(".claude/settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(
        &settings,
        r#"{"permissions":{"deny":["Read(./secrets.pem)"],"allow":["Bash(ls:*)"]}}"#,
    )
    .unwrap();

    write_all(&temp.0, &schema);

    let parsed: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    let deny = parsed["permissions"]["deny"].as_array().unwrap();
    assert_eq!(deny[0], "Read(./secrets.pem)");
    assert!(deny.iter().any(|d| d == "Read(./.env)"));
    assert_eq!(parsed["permissions"]["allow"][0], "Bash(ls:*)");
}
