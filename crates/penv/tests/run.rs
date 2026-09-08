//! `run` end to end: the binary spawns a real child, injects the values into it
//! and scrubs what comes back. Everything else about `run` is a unit test.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const SECRET: &str = "sk_test_FAKE0000";

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
    format!("# @penv=acme/api-gateway @schema=1\n\n{KEYS}")
}

#[cfg(windows)]
const SHELL: &str = "cmd";
#[cfg(windows)]
const SHELL_FLAG: &str = "/C";
#[cfg(windows)]
const ECHO_VALUES: &str = "echo %STRIPE_SECRET_KEY% %PORT%";

#[cfg(not(windows))]
const SHELL: &str = "sh";
#[cfg(not(windows))]
const SHELL_FLAG: &str = "-c";
#[cfg(not(windows))]
const ECHO_VALUES: &str = "echo $STRIPE_SECRET_KEY $PORT";

const EXIT_SEVEN: &str = "exit 7";

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new(files: &[(&str, &str)]) -> Workspace {
        let name = format!(
            "penv-run-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        for (file, contents) in files {
            std::fs::write(dir.join(file), contents).expect("a scratch file");
        }
        Workspace(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// `penv --agent run [args] -- <shell> <flag> <script>`.
    fn run(&self, args: &[&str], script: &str) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_penv"));
        command.current_dir(&self.0).arg("--agent").arg("run");
        command.args(args);
        command.arg("--").arg(SHELL).arg(SHELL_FLAG).arg(script);
        command.output().expect("penv runs")
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

#[test]
fn the_values_reach_the_child_and_the_secret_does_not_come_back() {
    let workspace = Workspace::new(&[
        (".env.schema", &local_schema()),
        (".env", &format!("STRIPE_SECRET_KEY={SECRET}\n")),
    ]);
    let output = workspace.run(&[], ECHO_VALUES);

    let text = stdout(&output);
    assert!(!text.contains(SECRET), "the value came back: {text}");
    assert!(text.contains("sk\u{2592}\u{2592}"), "not masked: {text}");
    assert!(text.contains("3000"), "the default never arrived: {text}");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
}

#[test]
fn the_childs_exit_code_is_this_commands_exit_code() {
    let workspace = Workspace::new(&[
        (".env.schema", &local_schema()),
        (".env", &format!("STRIPE_SECRET_KEY={SECRET}\n")),
    ]);
    assert_eq!(workspace.run(&[], EXIT_SEVEN).status.code(), Some(7));
}

#[test]
fn no_mask_is_ignored_when_an_agent_is_driving() {
    let workspace = Workspace::new(&[
        (".env.schema", &local_schema()),
        (".env", &format!("STRIPE_SECRET_KEY={SECRET}\n")),
    ]);
    let output = workspace.run(&["--no-mask"], ECHO_VALUES);

    assert!(!stdout(&output).contains(SECRET));
    assert!(
        stderr(&output).contains("--no-mask"),
        "no note was printed: {}",
        stderr(&output)
    );
}

#[test]
fn a_missing_value_stops_the_child_with_exit_three() {
    let workspace = Workspace::new(&[(".env.schema", &local_schema()), (".env", "PORT=3000\n")]);
    let output = workspace.run(&[], ECHO_VALUES);

    assert_eq!(output.status.code(), Some(3));
    let text = stdout(&output);
    assert!(text.contains("STRIPE_SECRET_KEY"), "{text}");
    assert!(text.contains("\"violations\""), "{text}");
}

#[test]
fn another_environment_is_refused_with_exit_six() {
    let workspace = Workspace::new(&[
        (".env.schema", &local_schema()),
        (".env", &format!("STRIPE_SECRET_KEY={SECRET}\n")),
    ]);
    let output = workspace.run(&["--env", "production"], ECHO_VALUES);

    assert_eq!(output.status.code(), Some(6));
    assert!(stderr(&output).contains("cloud"), "{}", stderr(&output));
}

#[test]
fn a_dotenv_with_no_schema_gets_one_written_and_then_runs() {
    let workspace =
        Workspace::new(&[(".env", &format!("STRIPE_SECRET_KEY={SECRET}\nPORT=3000\n"))]);
    let output = workspace.run(&[], ECHO_VALUES);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(workspace.path().join(".env.schema").is_file());
    assert!(
        stderr(&output).contains("penv init"),
        "init said nothing: {}",
        stderr(&output)
    );
    assert!(!stdout(&output).contains(SECRET));
}

#[test]
fn a_cloud_schema_with_no_local_values_names_the_phase_it_needs() {
    let cloud = cloud_schema();
    let workspace = Workspace::new(&[(".env.schema", &cloud)]);
    let output = workspace.run(&[], ECHO_VALUES);

    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("not_implemented"), "{text}");
    assert!(text.contains("phase 2"), "{text}");
}

#[test]
fn a_cloud_schema_with_a_local_dotenv_runs_and_says_so() {
    let cloud = cloud_schema();
    let workspace = Workspace::new(&[
        (".env.schema", &cloud),
        (".env", &format!("STRIPE_SECRET_KEY={SECRET}\n")),
    ]);
    let output = workspace.run(&[], ECHO_VALUES);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("cloud project"),
        "no warning: {}",
        stderr(&output)
    );
    assert!(!stdout(&output).contains(SECRET));
}
