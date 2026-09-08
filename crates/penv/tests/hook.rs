//! The hook matcher, as a table. It is a pure function of the request, so the
//! whole policy is readable here.

use penv::commands::hook::{
    DUMPS_ENV, Decision, READS_ENV, REVEALS_VALUE, Request, decide, extract,
};

fn on(command: &str) -> Decision {
    decide(&Request::command(command))
}

fn denied(command: &str, reason: &'static str) {
    assert_eq!(on(command), Decision::Deny(reason), "{command}");
}

fn allowed(command: &str) {
    assert_eq!(on(command), Decision::Allow, "{command}");
}

#[test]
fn reading_a_value_file_is_refused() {
    for command in [
        "cat .env",
        "cat ./.env",
        "cat /repo/.env.production",
        "type .\\.env.local",
        "grep DATABASE_URL .env",
        "head -n 5 .env.staging.local",
        "cp .env /tmp/x",
        "sed -n 1p apps/api/.env",
        "cat --file=.env",
        "cat \".env\"",
        "echo hi > .env",
    ] {
        denied(command, READS_ENV);
    }
}

#[test]
fn the_schema_stays_readable_because_it_holds_no_values() {
    for command in [
        "cat .env.schema",
        "grep PORT .env.schema",
        "cat ./.env.schema",
        "penv check",
        "penv ls",
        "penv gen ts",
        "cat package.json",
        "node scripts/build.js",
    ] {
        allowed(command);
    }
}

#[test]
fn dumping_the_environment_is_refused() {
    for command in [
        "printenv",
        "printenv | sort",
        "printenv DATABASE_URL",
        "env",
        "env -0",
        "set",
        "Get-ChildItem Env:",
        "gci env:",
        "ls Env:",
        "node -e \"console.log(process.env)\"",
        "node -p process.env",
        "python3 -c \"import os; print(os.environ)\"",
    ] {
        denied(command, DUMPS_ENV);
    }
}

#[test]
fn running_a_program_with_the_environment_is_not_dumping_it() {
    for command in [
        "env NODE_ENV=test npm run build",
        "set -e",
        "ls src",
        "grep -e process.env -r src",
        "npm set registry https://example.test",
    ] {
        allowed(command);
    }
}

#[test]
fn the_two_commands_that_print_values_are_refused() {
    for command in [
        "penv reveal STRIPE_SECRET_KEY",
        "penv pull",
        "penv --json pull",
        "/usr/local/bin/penv reveal PORT",
    ] {
        denied(command, REVEALS_VALUE);
    }
    allowed("penv push");
}

#[test]
fn a_read_of_a_value_file_by_path_is_refused() {
    assert_eq!(
        decide(&Request::path("/repo/.env.local")),
        Decision::Deny(READS_ENV)
    );
    assert_eq!(decide(&Request::path("/repo/.env.schema")), Decision::Allow);
    assert_eq!(decide(&Request::default()), Decision::Allow);
}

#[test]
fn one_refusal_in_a_chain_refuses_the_chain() {
    denied("npm run build && cat .env", READS_ENV);
    denied("mkdir x; printenv", DUMPS_ENV);
    denied("ls | cat .env", READS_ENV);
}

#[test]
fn the_claude_code_payload_reaches_the_matcher() {
    let request = extract(r#"{"tool_name":"Bash","tool_input":{"command":"cat .env"}}"#);
    assert_eq!(request.command.as_deref(), Some("cat .env"));
    assert_eq!(decide(&request), Decision::Deny(READS_ENV));
}

#[test]
fn the_cursor_payload_reaches_the_matcher() {
    let request = extract(r#"{"hook_event_name":"beforeReadFile","file_path":"/repo/.env"}"#);
    assert_eq!(request.path.as_deref(), Some("/repo/.env"));
    assert_eq!(decide(&request), Decision::Deny(READS_ENV));
}

#[test]
fn an_undocumented_payload_is_read_for_what_it_has() {
    assert_eq!(
        decide(&extract(r#"{"args":{"cmd":"printenv"}}"#)),
        Decision::Deny(DUMPS_ENV)
    );
    assert_eq!(
        decide(&extract("cat .env")),
        Decision::Deny(READS_ENV),
        "plain text falls back to the command"
    );
    assert_eq!(decide(&extract("")), Decision::Allow);
    assert_eq!(
        decide(&extract("{not json")),
        Decision::Allow,
        "a payload with nothing to match on is allowed, not blocked"
    );
}

/// The matcher reads a command as text. These get through, and the design says
/// so: detection changes defaults and friction, it is never the last line of
/// defence. The sandbox deny rules and short-lived cloud values are.
#[test]
fn the_known_evasions_are_known() {
    for command in [
        "echo Y2F0IC5lbnYK | base64 -d | sh",
        "n=env; cat \".$n\"",
        "cat .e*",
        "python3 -c \"print(open('.en' + 'v').read())\"",
        "node -e \"console.log(require('fs').readFileSync('.env','utf8'))\"",
    ] {
        allowed(command);
    }
}
