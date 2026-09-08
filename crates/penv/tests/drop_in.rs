//! The parts that are folders, end to end: a target nobody compiled in, the
//! guard states, and the hook answering in the shape its folder declares.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

const SCHEMA: &str = "\
# @schema=1

# @type=port @sensitive=false
PORT=3000
";

const GO_TARGET: &str = r#"
name = "go"
output = "env.go"
detect = ["go.mod"]

[types]
string = "string"
number = "float64"
integer = "int"
boolean = "bool"
url = "string"
email = "string"
port = "int"
enum = "'string'"
"#;

const GO_TEMPLATE: &str =
    "package env\n{% for key in keys %}// {{ key.name }} {{ key.lang_type }}\n{% endfor %}";

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new(files: &[(&str, &str)]) -> Workspace {
        let name = format!(
            "penv-drop-in-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let workspace = Workspace(dir);
        for (file, contents) in files {
            workspace.write(file, contents);
        }
        workspace
    }

    fn write(&self, file: &str, contents: &str) {
        let path = self.0.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a scratch directory");
        }
        std::fs::write(path, contents).expect("a scratch file");
    }

    fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }

    /// The binary with nothing of this machine around it: no PATH to find a
    /// harness on and a home directory of its own.
    fn penv(&self, args: &[&str]) -> Output {
        self.spawn(args, None)
    }

    fn hook(&self, args: &[&str], payload: &str) -> Output {
        self.spawn(args, Some(payload))
    }

    fn spawn(&self, args: &[&str], payload: Option<&str>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_penv"));
        command
            .current_dir(&self.0)
            .args(args)
            .env("PATH", "")
            .env("HOME", &self.0)
            .env("USERPROFILE", &self.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("penv runs");
        let mut stdin = child.stdin.take().expect("a pipe");
        let payload = payload.unwrap_or_default().to_string();
        std::thread::spawn(move || {
            let _ = stdin.write_all(payload.as_bytes());
        });
        child.wait_with_output().expect("penv finishes")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn json(output: &Output) -> Value {
    serde_json::from_str(&stdout(output))
        .unwrap_or_else(|e| panic!("not JSON: {e}\n{}\n{}", stdout(output), stderr(output)))
}

fn status(report: &Value, harness: &str) -> String {
    report["harnesses"]
        .as_array()
        .expect("harnesses")
        .iter()
        .find(|h| h["name"] == harness)
        .unwrap_or_else(|| panic!("no {harness} row in {report}"))["files"][0]["status"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn init_generates_a_target_that_is_only_a_folder() {
    let workspace = Workspace::new(&[
        (".env", "PORT=3000\n"),
        ("go.mod", "module example.test\n"),
        (".penv/targets/go/target.toml", GO_TARGET),
        (".penv/targets/go/env.tmpl", GO_TEMPLATE),
    ]);
    let report = json(&workspace.penv(&["--json", "init"]));

    let generated = report["generated"].as_array().expect("generated");
    assert!(
        generated
            .iter()
            .any(|p| p.as_str().unwrap().ends_with("env.go")),
        "the dropped-in target was skipped: {report}"
    );
    let written = std::fs::read_to_string(workspace.path("env.go")).expect("env.go");
    assert!(written.contains("// PORT int"), "{written}");
}

#[test]
fn a_target_the_repository_does_not_use_is_left_alone() {
    let workspace = Workspace::new(&[
        (".env", "PORT=3000\n"),
        (".penv/targets/go/target.toml", GO_TARGET),
        (".penv/targets/go/env.tmpl", GO_TEMPLATE),
    ]);
    workspace.penv(&["--json", "init"]);
    assert!(!workspace.path("env.go").exists(), "go.mod is not here");
}

#[test]
fn guard_names_the_same_three_states_whether_it_writes_or_checks() {
    let workspace = Workspace::new(&[(".env.schema", SCHEMA), (".claude/settings.json", "{}")]);

    let first = workspace.penv(&["--json", "guard", "--check"]);
    assert_eq!(status(&json(&first), "claude-code"), "stale");
    assert_eq!(first.status.code(), Some(3), "{}", stderr(&first));

    let written = workspace.penv(&["--json", "guard"]);
    assert_eq!(status(&json(&written), "claude-code"), "stale");
    assert_eq!(written.status.code(), Some(0), "{}", stderr(&written));

    let again = workspace.penv(&["--json", "guard", "--check"]);
    assert_eq!(status(&json(&again), "claude-code"), "current");
    assert_eq!(again.status.code(), Some(0));

    let settings = std::fs::read_to_string(workspace.path(".claude/settings.json")).unwrap();
    assert!(settings.contains("Read(./.env.*)"), "{settings}");
}

#[test]
fn a_missing_file_reads_as_missing_and_never_as_written() {
    let workspace = Workspace::new(&[(".env.schema", SCHEMA), (".cursor/cli.json", "{}")]);
    let report = json(&workspace.penv(&["--json", "guard", "--check", "cursor"]));
    let files = report["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["name"] == "cursor")
        .unwrap()["files"]
        .clone();
    assert_eq!(files[0]["status"], "stale", "{files}");
    assert_eq!(files[1]["status"], "missing", "{files}");
}

#[test]
fn a_laptop_with_no_harness_is_not_a_failing_one() {
    let workspace = Workspace::new(&[(".env.schema", SCHEMA)]);
    let output = workspace.penv(&["--json", "guard", "--check"]);
    let report = json(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        report["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["files"].as_array().unwrap().is_empty()),
        "a harness nobody installed was reported on: {report}"
    );
    assert!(!stdout(&output).contains("skipped"));
}

#[test]
fn the_hook_answers_in_the_shape_its_folder_declares() {
    let workspace = Workspace::new(&[(".env.schema", SCHEMA)]);

    let claude = workspace.hook(
        &["hook", "claude-code"],
        r#"{"tool_name":"Read","tool_input":{"file_path":".env"}}"#,
    );
    let answer = json(&claude);
    assert_eq!(claude.status.code(), Some(0), "{}", stderr(&claude));
    assert_eq!(
        answer["hookSpecificOutput"]["permissionDecision"], "deny",
        "{answer}"
    );

    let cursor = workspace.hook(&["hook", "cursor"], r#"{"command":"printenv"}"#);
    let answer = json(&cursor);
    assert_eq!(answer["permission"], "deny");
    assert_eq!(answer["userMessage"], answer["user_message"]);

    let unknown = workspace.hook(&["hook", "nano"], r#"{"command":"printenv"}"#);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(
        stderr(&unknown).contains("penv blocks dumping"),
        "{}",
        stderr(&unknown)
    );
    assert!(stdout(&unknown).trim().is_empty());
}

#[test]
fn a_hook_that_has_nothing_to_refuse_says_nothing() {
    let workspace = Workspace::new(&[(".env.schema", SCHEMA)]);
    for payload in [
        "",
        r#"{"tool_name":"Bash","tool_input":{"command":"ls src"}}"#,
    ] {
        let output = workspace.hook(&["hook", "claude-code"], payload);
        assert_eq!(output.status.code(), Some(0));
        assert!(stdout(&output).trim().is_empty(), "{}", stdout(&output));
    }
}

#[test]
fn bare_penv_says_the_state_and_nothing_about_credentials() {
    let workspace = Workspace::new(&[(".env.schema", SCHEMA)]);
    let output = workspace.penv(&["--json"]);
    let report = json(&output);
    assert_eq!(report["state"], "local");
    assert_eq!(report["next"], "penv check");
    assert!(report["policy"].is_null(), "{report}");
    assert!(!stdout(&output).contains("credentialTtlSecs"));
}

#[test]
fn a_printed_path_uses_one_separator() {
    let workspace = Workspace::new(&[(".env", "PORT=3000\n"), (".claude/settings.json", "{}")]);
    let report = json(&workspace.penv(&["--json", "init"]));
    for path in report["guarded"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(report["generated"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
    {
        let mixed = path.contains('/') && path.contains('\\');
        assert!(!mixed, "{path} mixes separators");
        assert!(
            path.contains(std::path::MAIN_SEPARATOR),
            "{path} is not written the way this platform writes one"
        );
    }
}
