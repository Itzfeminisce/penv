use std::io::Read;

use serde_json::{Value, json};

use crate::error::{CliError, Exit};
use crate::output::Report;

pub const READS_ENV: &str = "penv blocks reads of .env files. Run penv ls for the key names and penv check for what is missing; .env.schema is readable.";
pub const DUMPS_ENV: &str = "penv blocks dumping the environment, because it prints every value. Run penv ls for the key names.";
pub const REVEALS_VALUE: &str =
    "penv reveal and penv pull print values, so they are refused in an agent session.";

/// What a harness handed the hook, reduced to the two things that matter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Request {
    pub command: Option<String>,
    pub path: Option<String>,
}

impl Request {
    pub fn command(command: &str) -> Request {
        Request {
            command: Some(command.to_string()),
            path: None,
        }
    }

    pub fn path(path: &str) -> Request {
        Request {
            command: None,
            path: Some(path.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(&'static str),
}

/// Read the harness payload from stdin, decide, and answer in that harness's own
/// shape. Anything this cannot parse is allowed: a hook that fails closed on a
/// surprise stops the agent working.
pub fn run(harness: &str) -> Result<Report, CliError> {
    let mut payload = String::new();
    let _ = std::io::stdin().read_to_string(&mut payload);

    let Decision::Deny(reason) = decide(&extract(&payload)) else {
        return Ok(Report::silent());
    };

    match harness {
        "claude-code" => Ok(answer(json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }
        }))),
        "cursor" => Ok(answer(
            json!({ "permission": "deny", "userMessage": reason }),
        )),
        // Every other harness reads a blocked call as exit 2 with a reason on
        // stderr; 2 is that convention, not penv's auth code.
        _ => {
            eprintln!("{reason}");
            Ok(Report::silent().with_exit(Exit::Auth))
        }
    }
}

fn answer(body: Value) -> Report {
    let text = body.to_string();
    Report::new(body, text)
}

/// Pull a command and a path out of whatever JSON the harness sent. Documented
/// shapes nest them; the rest are found by name.
pub fn extract(payload: &str) -> Request {
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        let text = payload.trim();
        return if text.is_empty() {
            Request::default()
        } else {
            Request::command(text)
        };
    };
    let mut request = Request::default();
    walk(&value, &mut request);
    request
}

fn walk(value: &Value, request: &mut Request) {
    match value {
        Value::Object(fields) => {
            for (name, child) in fields {
                if let Value::String(text) = child {
                    match name.as_str() {
                        "command" | "cmd" if request.command.is_none() => {
                            request.command = Some(text.clone());
                        }
                        "file_path" | "filePath" | "path" | "file" if request.path.is_none() => {
                            request.path = Some(text.clone());
                        }
                        _ => {}
                    }
                }
                walk(child, request);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| walk(item, request)),
        _ => {}
    }
}

/// The whole policy, as a function of the request. No I/O, no environment.
pub fn decide(request: &Request) -> Decision {
    if request.path.as_deref().is_some_and(touches_env_file) {
        return Decision::Deny(READS_ENV);
    }
    let Some(command) = &request.command else {
        return Decision::Allow;
    };
    // A quoted snippet carries its own separators, so it is read whole before
    // the line is split into the things that actually run.
    if inline_snippet_prints_the_environment(&words(command)) {
        return Decision::Deny(DUMPS_ENV);
    }
    for segment in segments(command) {
        if let Some(reason) = refuse(&segment) {
            return Decision::Deny(reason);
        }
    }
    Decision::Allow
}

/// True for `.env` and `.env.<anything>`, and false for `.env.schema`, which
/// holds no values and is the file an agent needs most.
pub fn touches_env_file(candidate: &str) -> bool {
    candidate.split('=').any(|piece| {
        let piece = piece.trim_matches(['"', '\'', '`', '(', ')', '<', '>']);
        let name = piece.rsplit(['/', '\\']).next().unwrap_or(piece);
        name != ".env.schema" && (name == ".env" || name.starts_with(".env."))
    })
}

fn refuse(segment: &str) -> Option<&'static str> {
    let words = words(segment);
    if words.iter().any(|w| touches_env_file(w)) {
        return Some(READS_ENV);
    }
    if dumps_environment(&words) {
        return Some(DUMPS_ENV);
    }
    if reveals_a_value(&words) {
        return Some(REVEALS_VALUE);
    }
    None
}

fn dumps_environment(words: &[String]) -> bool {
    let rest = skip_assignments(words);
    let Some(first) = rest.first().map(|w| leaf(w)) else {
        return false;
    };
    let arguments = &rest[1..];

    // printenv only ever prints values; env and set only when nothing follows.
    if first == "printenv" {
        return true;
    }
    if first == "env" && arguments.iter().all(|a| a.starts_with('-')) {
        return true;
    }
    if first == "set" && arguments.is_empty() {
        return true;
    }
    if matches!(
        first.to_lowercase().as_str(),
        "get-childitem" | "gci" | "ls" | "dir" | "get-item" | "gi"
    ) && arguments
        .iter()
        .any(|a| a.to_lowercase().starts_with("env:"))
    {
        return true;
    }
    false
}

/// `node -e process.env`, `python -c os.environ`. An interpreter and an inline
/// flag have to be there too, so `grep -e process.env` stays a search.
fn inline_snippet_prints_the_environment(words: &[String]) -> bool {
    let interpreter = words.iter().any(|w| {
        matches!(
            leaf(w).as_str(),
            "node"
                | "node.exe"
                | "deno"
                | "bun"
                | "python"
                | "python3"
                | "python.exe"
                | "py"
                | "ruby"
                | "perl"
                | "sh"
                | "bash"
                | "zsh"
                | "pwsh"
                | "powershell"
                | "powershell.exe"
        )
    });
    let flagged = words.iter().any(|w| {
        matches!(
            w.as_str(),
            "-c" | "-e" | "-p" | "--eval" | "--print" | "--command"
        )
    });
    interpreter
        && flagged
        && words
            .iter()
            .any(|w| w.contains("process.env") || w.contains("os.environ"))
}

fn reveals_a_value(words: &[String]) -> bool {
    let rest = skip_assignments(words);
    let Some(first) = rest.first().map(|w| leaf(w)) else {
        return false;
    };
    if first != "penv" && first != "penv.exe" {
        return false;
    }
    rest[1..]
        .iter()
        .find(|w| !w.starts_with('-'))
        .is_some_and(|w| w == "reveal" || w == "pull")
}

fn skip_assignments(words: &[String]) -> &[String] {
    let start = words
        .iter()
        .position(|w| !w.contains('=') || w.starts_with('-'))
        .unwrap_or(words.len());
    &words[start..]
}

fn leaf(word: &str) -> String {
    word.rsplit(['/', '\\'])
        .next()
        .unwrap_or(word)
        .to_lowercase()
}

/// One shell line per thing that actually runs.
fn segments(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ';' | '\n' | '|' | '&' => {
                if (c == '|' || c == '&') && chars.peek() == Some(&c) {
                    chars.next();
                }
                out.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
    }
    out.push(current);
    out.into_iter()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .collect()
}

fn words(segment: &str) -> Vec<String> {
    segment
        .split_whitespace()
        .map(|word| {
            word.trim_matches(['"', '\'', '`', '(', ')'])
                .trim_start_matches(['<', '>'])
                .to_string()
        })
        .filter(|word| !word.is_empty())
        .collect()
}
