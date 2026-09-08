use penv::cli::Format;
use penv::env::Env;
use penv::error::{CliError, Exit};
use penv::output::{Output, Render, Report, resolve, table};
use serde_json::json;

const TTY: bool = true;
const PIPE: bool = false;

fn env(pairs: &[(&str, &str)]) -> Env {
    Env::from_pairs(pairs)
}

#[test]
fn a_terminal_gets_text_with_colour() {
    assert_eq!(
        resolve(false, None, false, TTY, &env(&[])),
        Render {
            json: false,
            color: true
        }
    );
}

#[test]
fn a_pipe_gets_json() {
    assert_eq!(
        resolve(false, None, false, PIPE, &env(&[])),
        Render {
            json: true,
            color: false
        }
    );
}

#[test]
fn the_json_flag_and_the_agent_flag_both_force_json() {
    assert!(resolve(true, None, false, TTY, &env(&[])).json);
    assert!(resolve(false, None, true, TTY, &env(&[])).json);
}

#[test]
fn the_format_flag_beats_what_stdout_is_attached_to() {
    assert!(!resolve(false, Some(Format::Text), false, PIPE, &env(&[])).json);
    assert!(resolve(false, Some(Format::Json), false, TTY, &env(&[])).json);
}

#[test]
fn json_is_never_coloured() {
    for render in [
        resolve(true, None, false, TTY, &env(&[])),
        resolve(false, None, true, TTY, &env(&[])),
        resolve(false, None, false, PIPE, &env(&[])),
    ] {
        assert!(!render.color);
    }
}

#[test]
fn no_color_and_clicolor_are_honoured() {
    assert!(!resolve(false, None, false, TTY, &env(&[("NO_COLOR", "1")])).color);
    assert!(!resolve(false, None, false, TTY, &env(&[("CLICOLOR", "0")])).color);
    assert!(
        resolve(false, None, false, TTY, &env(&[("NO_COLOR", "")])).color,
        "an empty NO_COLOR does not opt out"
    );
    assert!(resolve(false, None, false, TTY, &env(&[("CLICOLOR", "1")])).color);
}

fn rendered(render: Render, report: &Report) -> String {
    let mut buffer = Vec::new();
    Output::new(render).write(report, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

#[test]
fn a_report_renders_as_text_or_as_json() {
    let report = Report::new(json!({ "state": "local" }), "state   local");
    assert_eq!(
        rendered(
            Render {
                json: false,
                color: false
            },
            &report
        ),
        "state   local\n"
    );
    let json = rendered(
        Render {
            json: true,
            color: false,
        },
        &report,
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        report.json
    );
}

fn failed(render: Render, error: &CliError) -> String {
    let mut buffer = Vec::new();
    Output::new(render).fail(error, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

#[test]
fn an_error_is_one_json_object_or_one_line() {
    let error = CliError::new(
        "no_dotenv",
        "there is no .env in this directory.",
        "Write a .env with your keys, then run penv init.",
    );

    let json = failed(
        Render {
            json: true,
            color: false,
        },
        &error,
    );
    assert_eq!(json.lines().count(), 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        json!({
            "error": "no_dotenv",
            "message": "there is no .env in this directory.",
            "fix": "Write a .env with your keys, then run penv init.",
        })
    );

    let text = failed(
        Render {
            json: false,
            color: false,
        },
        &error,
    );
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("there is no .env"));
    assert!(!text.contains('\u{1b}'), "colour was not asked for");

    let coloured = failed(
        Render {
            json: false,
            color: true,
        },
        &error,
    );
    assert!(coloured.contains('\u{1b}'));
}

#[test]
fn exit_codes_are_the_numbers_the_design_publishes() {
    assert_eq!(Exit::Ok as i32, 0);
    assert_eq!(Exit::Error as i32, 1);
    assert_eq!(Exit::Auth as i32, 2);
    assert_eq!(Exit::Validation as i32, 3);
    assert_eq!(Exit::Confirmation as i32, 4);
    assert_eq!(Exit::NoCredential as i32, 5);
    assert_eq!(Exit::EnvironmentRefused as i32, 6);
}

#[test]
fn a_stub_command_refuses_with_exit_one() {
    let error = CliError::not_in_this_build("push");
    assert_eq!(error.exit, Exit::Error);
    assert!(error.message.contains("penv push"));
}

#[test]
fn columns_line_up() {
    let style = Output::new(Render {
        json: false,
        color: false,
    })
    .style();
    let rows = vec![
        vec!["PORT".into(), "port".into()],
        vec!["DATABASE_URL".into(), "url".into()],
    ];
    let text = table(&["KEY", "TYPE"], &rows, &style);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "KEY           TYPE");
    assert_eq!(lines[1], "PORT          port");
    assert_eq!(lines[2], "DATABASE_URL  url");
}
