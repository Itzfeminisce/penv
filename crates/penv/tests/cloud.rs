//! Cloud mode end to end: the binary talks to an in-process server on a
//! loopback port, so the tests reach no network and no keychain.

// One mock server, kept where the client that speaks to it lives.
#[path = "../../penv-cloud/tests/common/mod.rs"]
mod server;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Value, json};
use server::Mock;

const SECRET: &str = "sk_test_FAKE0000";
const TOKEN: &str = "pck_FAKE";
const PROJECT: &str = "api-gateway";
const ENVS: &str = "/api/v1/envs/acme/api-gateway/development";

const KEYS: &str = "\
# @type=string(startsWith=sk_)
STRIPE_SECRET_KEY=

# @type=port @sensitive=false
PORT=3000
";

fn local_schema() -> String {
    format!("# @schema=1\n\n{KEYS}")
}

fn cloud_schema() -> String {
    format!("# @penv=acme/{PROJECT} @schema=1\n\n{KEYS}")
}

#[cfg(windows)]
const SHELL: [&str; 2] = ["cmd", "/C"];
#[cfg(windows)]
const ECHO_VALUES: &str = "echo %STRIPE_SECRET_KEY% %PORT%";

#[cfg(not(windows))]
const SHELL: [&str; 2] = ["sh", "-c"];
#[cfg(not(windows))]
const ECHO_VALUES: &str = "echo $STRIPE_SECRET_KEY $PORT";

/// Everything penv-agent looks at, so a test says who is driving.
const AGENT_MARKERS: [&str; 14] = [
    "AGENT",
    "AI_AGENT",
    "AMP_CURRENT_THREAD_ID",
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLINE_ACTIVE",
    "CODEX_SESSION_ID",
    "CODEX_THREAD_ID",
    "COPILOT_CLI",
    "CURSOR_AGENT",
    "CURSOR_SANDBOX",
    "GEMINI_CLI",
    "ROO_ACTIVE",
];

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A scratch project directory. Its name is the project name `push` offers.
struct Workspace {
    root: PathBuf,
    dir: PathBuf,
}

impl Workspace {
    fn new(files: &[(&str, &str)]) -> Workspace {
        let root = std::env::temp_dir().join(format!(
            "penv-cloud-cli-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let dir = root.join(PROJECT);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        for (file, contents) in files {
            std::fs::write(dir.join(file), contents).expect("a scratch file");
        }
        Workspace { root, dir }
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn read(&self, file: &str) -> String {
        std::fs::read_to_string(self.dir.join(file)).unwrap_or_default()
    }

    /// The binary, pointed at the mock, with a credential in the environment and
    /// no cache directory, so nothing on this host is read or written.
    fn command(&self, mock: &Mock) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_penv"));
        command
            .current_dir(&self.dir)
            .env("PENV_URL", mock.url())
            .env("PENV_TOKEN", TOKEN)
            .env_remove("PENV_ENV")
            .env_remove("LOCALAPPDATA")
            .env_remove("XDG_CACHE_HOME")
            .env_remove("HOME");
        // The suite itself may be running under an agent, and these tests decide
        // for themselves which sessions are one.
        for marker in AGENT_MARKERS {
            command.env_remove(marker);
        }
        command
    }

    fn run(&self, mock: &Mock, args: &[&str]) -> Output {
        self.command(mock).args(args).output().expect("penv runs")
    }

    fn pipe(&self, mock: &Mock, args: &[&str], stdin: &str) -> Output {
        use std::io::Write;

        let mut child = self
            .command(mock)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("penv runs");
        child
            .stdin
            .take()
            .expect("a pipe")
            .write_all(stdin.as_bytes())
            .expect("the value goes in");
        child.wait_with_output().expect("penv finishes")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn json_of(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

fn values_body() -> String {
    json!({
        "keys": [
            { "path": "", "name": "STRIPE_SECRET_KEY", "kind": "static", "version": 2, "value": SECRET },
            { "path": "", "name": "PORT", "kind": "static", "version": 1, "value": "3000" },
        ]
    })
    .to_string()
}

// --- push -------------------------------------------------------------------

#[test]
fn push_creates_the_project_from_the_directory_and_writes_the_header() {
    let mock = Mock::new();
    mock.on(
        "GET",
        "/api/v1/orgs",
        200,
        &json!({ "orgs": [{ "slug": "acme", "name": "Acme" }] }).to_string(),
    );
    mock.on("POST", "/api/v1/orgs/acme/projects", 201, "{}");
    mock.on(
        "PUT",
        ENVS,
        200,
        &json!({ "written": 2, "unchanged": 0, "pruned": 0, "etag": "\"abc\"" }).to_string(),
    );

    let workspace = Workspace::new(&[
        (".env.schema", &local_schema()),
        (".env", &format!("STRIPE_SECRET_KEY={SECRET}\n")),
    ]);
    let output = workspace.run(&mock, &["--json", "push"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));

    let report = json_of(&stdout(&output));
    assert_eq!(report["created"], format!("acme/{PROJECT}"));
    assert_eq!(report["written"], 2);
    assert_eq!(report["etag"], "\"abc\"");

    let created = mock.last("POST", "/api/v1/orgs/acme/projects").json();
    assert_eq!(created["name"], PROJECT);
    assert_eq!(created["environments"][0], "development");

    let put = mock.last("PUT", ENVS).json();
    assert_eq!(put["prune"], false);
    let sent = put["keys"].as_array().unwrap();
    let secret = sent
        .iter()
        .find(|k| k["name"] == "STRIPE_SECRET_KEY")
        .unwrap();
    assert_eq!(secret["value"], SECRET);
    assert_eq!(secret["schema"]["type"]["name"], "string");
    assert_eq!(
        secret["schema"].get("name"),
        None,
        "the route carries the name"
    );

    assert!(
        workspace
            .read(".env.schema")
            .contains(&format!("@penv=acme/{PROJECT}")),
        "the header was not written: {}",
        workspace.read(".env.schema")
    );
    assert!(
        !workspace.path().join(".env").exists(),
        ".env was left behind"
    );
}

#[test]
fn push_says_which_organisations_it_could_not_choose_between() {
    let mock = Mock::new();
    mock.on(
        "GET",
        "/api/v1/orgs",
        200,
        &json!({ "orgs": [{ "slug": "acme", "name": "Acme" }, { "slug": "other", "name": "Other" }] })
            .to_string(),
    );
    let workspace = Workspace::new(&[(".env.schema", &local_schema()), (".env", "PORT=3000\n")]);
    let output = workspace.run(&mock, &["--json", "push"]);

    assert_eq!(output.status.code(), Some(1));
    let error = json_of(&stderr(&output));
    assert_eq!(error["error"], "org_required");
    assert!(
        error["message"].as_str().unwrap().contains("acme"),
        "{error}"
    );
    assert!(error["fix"].as_str().unwrap().contains("--org"), "{error}");
}

// --- pull -------------------------------------------------------------------

#[test]
fn pull_is_refused_for_an_agent_and_written_for_a_person() {
    let mock = Mock::new();
    mock.on("GET", ENVS, 200, &values_body());
    let workspace = Workspace::new(&[(".env.schema", &cloud_schema())]);

    let refused = workspace.run(&mock, &["--agent", "pull"]);
    assert_eq!(refused.status.code(), Some(2));
    assert_eq!(json_of(&stderr(&refused))["error"], "agent_session");
    assert!(!workspace.path().join(".env").exists());
    assert!(mock.hits("GET", ENVS).is_empty(), "it never asked");

    let allowed = workspace.run(&mock, &["--json", "pull", "--i-am-human"]);
    assert_eq!(allowed.status.code(), Some(0), "{}", stderr(&allowed));
    let report = json_of(&stdout(&allowed));
    assert_eq!(report["keys"], 2);
    assert!(
        !stdout(&allowed).contains(SECRET),
        "pull prints the count, not the values"
    );
    assert_eq!(
        workspace.read(".env"),
        format!("STRIPE_SECRET_KEY={SECRET}\nPORT=3000\n")
    );
}

// --- set and unset ----------------------------------------------------------

#[test]
fn set_takes_a_piped_value_echoes_nothing_and_declares_the_new_key() {
    let mock = Mock::new();
    mock.on(
        "PATCH",
        &format!("{ENVS}/keys/NEW_TOKEN"),
        200,
        &json!({ "version": 1, "etag": "\"abc\"" }).to_string(),
    );
    let workspace = Workspace::new(&[(".env.schema", &cloud_schema())]);
    let output = workspace.pipe(&mock, &["--json", "set", "NEW_TOKEN"], "sk_live_FAKE1111\n");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let report = json_of(&stdout(&output));
    assert_eq!(report["key"], "NEW_TOKEN");
    assert_eq!(report["version"], 1);
    assert_eq!(report["schemaAdded"], true);

    assert!(!stdout(&output).contains("sk_live_FAKE1111"), "it echoed");
    assert!(!stderr(&output).contains("sk_live_FAKE1111"), "it echoed");
    assert!(
        !stderr(&output).contains("NEW_TOKEN:"),
        "a piped value is never prompted for: {}",
        stderr(&output)
    );

    assert_eq!(
        mock.last("PATCH", &format!("{ENVS}/keys/NEW_TOKEN")).json()["value"],
        "sk_live_FAKE1111"
    );
    let schema = workspace.read(".env.schema");
    assert!(schema.contains("NEW_TOKEN="), "{schema}");
    assert!(
        !schema.contains("sk_live_FAKE1111"),
        "the value landed: {schema}"
    );
    assert!(
        schema.contains("PORT=3000"),
        "the file was rewritten: {schema}"
    );
}

#[test]
fn set_refuses_a_value_the_shell_would_remember() {
    let mock = Mock::new();
    let workspace = Workspace::new(&[(".env.schema", &cloud_schema())]);
    let output = workspace.run(&mock, &["--json", "set", "PORT", "--value", "3000"]);

    assert_eq!(output.status.code(), Some(1));
    let error = json_of(&stderr(&output));
    assert_eq!(error["error"], "value_on_the_command_line");
    assert!(mock.requests().is_empty(), "it never asked");
}

#[test]
fn unset_removes_the_value_and_keeps_the_block() {
    let mock = Mock::new();
    mock.on(
        "DELETE",
        &format!("{ENVS}/keys/PORT"),
        200,
        &json!({ "etag": "\"abc\"" }).to_string(),
    );
    let workspace = Workspace::new(&[(".env.schema", &cloud_schema())]);
    let output = workspace.run(&mock, &["--json", "unset", "PORT"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(json_of(&stdout(&output))["key"], "PORT");
    assert!(workspace.read(".env.schema").contains("PORT=3000"));
}

// --- reveal -----------------------------------------------------------------

#[test]
fn reveal_is_refused_in_an_agent_session() {
    let mock = Mock::new();
    mock.on("GET", ENVS, 200, &values_body());
    let workspace = Workspace::new(&[(".env.schema", &cloud_schema())]);
    let output = workspace.run(&mock, &["--agent", "reveal", "STRIPE_SECRET_KEY"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json_of(&stderr(&output))["error"], "agent_session");
    assert!(!stdout(&output).contains(SECRET));
    assert!(mock.hits("GET", ENVS).is_empty(), "it never asked");
}

#[test]
fn reveal_prints_the_one_value_and_nothing_else() {
    let mock = Mock::new();
    mock.on("GET", ENVS, 200, &values_body());
    let workspace = Workspace::new(&[(".env.schema", &cloud_schema())]);

    let output = workspace.run(&mock, &["--json", "reveal", "PORT"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        json_of(&stdout(&output)),
        json!({ "key": "PORT", "value": "3000" })
    );

    let plain = workspace.run(&mock, &["--format", "text", "reveal", "PORT"]);
    assert_eq!(stdout(&plain).trim_end(), "3000");
}

// --- run --------------------------------------------------------------------

#[test]
fn run_injects_the_cloud_values_and_stamps_the_session_on_the_request() {
    let mock = Mock::new();
    mock.on("GET", ENVS, 200, &values_body());
    let workspace = Workspace::new(&[(".env.schema", &cloud_schema())]);

    let output = workspace
        .command(&mock)
        .env("CLAUDECODE", "1")
        .env("CLAUDE_CODE_SESSION_ID", "sess-9")
        .args(["run", "--"])
        .args(SHELL)
        .arg(ECHO_VALUES)
        .output()
        .expect("penv runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("3000"),
        "the public value never arrived: {text}"
    );
    assert!(!text.contains(SECRET), "the value came back: {text}");

    let request = mock.last("GET", ENVS);
    assert_eq!(request.header("x-penv-agent"), Some("claude-code"));
    assert_eq!(request.header("x-penv-session"), Some("sess-9"));
    assert_eq!(request.header("authorization"), Some("Bearer pck_FAKE"));
}

#[test]
fn run_maps_a_refused_environment_to_exit_six_and_names_it() {
    let mock = Mock::new();
    mock.on(
        "GET",
        "/api/v1/envs/acme/api-gateway/production",
        403,
        &json!({ "error": "forbidden" }).to_string(),
    );
    let workspace = Workspace::new(&[(".env.schema", &cloud_schema())]);

    let output = workspace
        .command(&mock)
        .args(["--agent", "run", "--env", "production", "--"])
        .args(SHELL)
        .arg(ECHO_VALUES)
        .output()
        .expect("penv runs");

    assert_eq!(output.status.code(), Some(6), "{}", stderr(&output));
    let error = json_of(&stderr(&output));
    assert_eq!(error["error"], "environment_refused");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("acme/api-gateway/production"),
        "{error}"
    );
}

#[test]
fn a_host_with_no_credential_says_so_with_exit_five() {
    let mock = Mock::new();
    let workspace = Workspace::new(&[(".env.schema", &cloud_schema())]);

    let output = workspace
        .command(&mock)
        .env_remove("PENV_TOKEN")
        .args(["--agent", "run", "--"])
        .args(SHELL)
        .arg(ECHO_VALUES)
        .output()
        .expect("penv runs");

    assert_eq!(output.status.code(), Some(5), "{}", stderr(&output));
    let error = json_of(&stderr(&output));
    assert_eq!(error["error"], "no_credential");
    assert!(
        error["fix"].as_str().unwrap().contains("penv login"),
        "{error}"
    );
    assert!(mock.requests().is_empty(), "it never asked");
}

// --- state ------------------------------------------------------------------

#[test]
fn bare_penv_reports_cloud_without_saying_which_credential() {
    let mock = Mock::new();
    let workspace = Workspace::new(&[(".env.schema", &cloud_schema())]);
    let output = workspace.run(&mock, &["--json"]);

    let report = json_of(&stdout(&output));
    assert_eq!(report["state"], "cloud");
    assert_eq!(report["project"], format!("acme/{PROJECT}"));
    assert_eq!(report["credential"], true);
    assert_eq!(report["next"], "penv run");
    assert!(report["cacheAge"].is_null(), "no cache directory, no cache");
    assert!(!stdout(&output).contains(TOKEN), "it named the credential");
}

#[test]
fn login_is_refused_in_an_agent_session() {
    let mock = Mock::new();
    let workspace = Workspace::new(&[(".env.schema", &cloud_schema())]);
    let output = workspace.run(&mock, &["--agent", "login"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json_of(&stderr(&output))["error"], "agent_session");
    assert!(mock.requests().is_empty(), "it never asked");
}

// --- machine ----------------------------------------------------------------

#[test]
fn machine_enroll_needs_a_secret() {
    let mock = Mock::new();
    let workspace = Workspace::new(&[(".env.schema", &cloud_schema())]);
    let output = workspace.run(&mock, &["--json", "machine", "enroll", ""]);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(json_of(&stderr(&output))["error"], "no_secret");
    assert!(mock.requests().is_empty(), "it never asked");
}
