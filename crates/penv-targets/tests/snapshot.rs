//! One fixture schema through every built-in target, byte for byte.

use penv_targets::{Roots, Source, Target, Tree, load, render};

const FIXTURE: &str = include_str!("fixture.env.schema");
const TS: &str = include_str!("snapshots/ts.env.ts");
const PY: &str = include_str!("snapshots/py.penv_env.py");

/// Pinned so a version bump is not a snapshot change.
const VERSION: &str = "1.0.0-test";

struct Empty;

impl Tree for Empty {
    fn read(&self, _path: &str) -> Option<String> {
        None
    }
    fn dirs(&self, _path: &str) -> Vec<String> {
        Vec::new()
    }
}

fn built_in(name: &str) -> Target {
    let target = load(&Empty, &Roots::new("/repo", None), name).unwrap();
    assert_eq!(target.source, Source::BuiltIn);
    target
}

fn rendered(name: &str) -> String {
    let schema = penv_schema::parse(FIXTURE).expect("the fixture parses");
    render(&built_in(name), &schema.to_json(), VERSION).expect("the fixture renders")
}

/// Point at the first line that differs; the whole file is too long to read in a
/// test failure.
fn assert_same(name: &str, expected: &str, actual: &str) {
    if expected == actual {
        return;
    }
    if std::env::var_os("PENV_BLESS").is_some() {
        std::fs::write(format!("tests/snapshots/{name}"), actual).unwrap();
        panic!("{name} was rewritten from PENV_BLESS; read the diff and run again");
    }
    let mut hint = String::new();
    for (line, (want, got)) in expected.lines().zip(actual.lines()).enumerate() {
        if want != got {
            hint = format!("line {}:\n-{want}\n+{got}", line + 1);
            break;
        }
    }
    if hint.is_empty() {
        hint = format!(
            "same prefix, {} expected line(s) against {} rendered",
            expected.lines().count(),
            actual.lines().count()
        );
    }
    panic!(
        "tests/snapshots/{name} no longer matches the fixture render.\n{hint}\nRe-run with PENV_BLESS=1 once the change is deliberate."
    );
}

#[test]
fn the_ts_target_renders_its_snapshot() {
    assert_same("ts.env.ts", TS, &rendered("ts"));
}

#[test]
fn the_py_target_renders_its_snapshot() {
    assert_same("py.penv_env.py", PY, &rendered("py"));
}

#[test]
fn a_constraint_never_reaches_the_generated_file() {
    for snapshot in [TS, PY] {
        assert!(
            !snapshot.contains("sk_"),
            "startsWith=sk_ leaked into the output"
        );
    }
}
