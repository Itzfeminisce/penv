use std::io::IsTerminal;
use std::path::Path;
use std::time::Duration;

use penv_cloud::Clock;
use penv_cloud::api::DevicePoll;
use serde_json::json;

use crate::agent::detect_here;
use crate::commands::cloud::{Cloud, note, open_browser, refuse};
use crate::env::Env;
use crate::error::{CliError, Exit};
use crate::output::{Output, Report};

/// Sign a person in with a device code. The credential goes to the OS keychain
/// and nowhere else.
pub fn run(out: &Output, _cwd: &Path, env: &Env, agent_flag: bool) -> Result<Report, CliError> {
    let tty = std::io::stdout().is_terminal();
    let detection = detect_here(env, tty);
    if detection.is_agent() || agent_flag {
        return Err(CliError::new(
            "agent_session",
            format!(
                "penv login signs a person in, and this session is {}.",
                detection.name().unwrap_or("an agent")
            ),
            "Sign in yourself, or give the agent a machine credential in PENV_TOKEN.",
        )
        .with_exit(Exit::Auth));
    }

    let cloud = Cloud::open(env, &detection)?;
    let start = cloud.api.device_start().map_err(|e| refuse(e, None))?;

    note(&format!("your code is {}", start.user_code));
    note(&format!("open {}", start.verification_uri));
    if tty && open_browser(&start.verification_uri) {
        note("a browser was opened for you");
    }

    let grant = poll(&cloud, &start)?;
    cloud
        .keychain
        .set(penv_cloud::keychain::USER, &grant.credential)
        .map_err(|e| refuse(e, None))?;

    let email = grant.user.as_ref().map(|u| u.email.clone());
    let orgs: Vec<String> = grant.orgs.iter().map(|o| o.slug.clone()).collect();
    let style = out.style();
    let text = format!(
        "{} {}\n{} {}",
        style.green("signed in"),
        email.clone().unwrap_or_else(|| "this account".into()),
        style.dim("orgs"),
        if orgs.is_empty() {
            "none yet".to_string()
        } else {
            orgs.join(", ")
        }
    );

    Ok(Report::new(
        json!({
            "signedIn": true,
            "email": email,
            "orgs": grant.orgs.iter().map(|o| json!({ "slug": o.slug, "name": o.name })).collect::<Vec<_>>(),
            "server": cloud.api.base_url(),
        }),
        text,
    ))
}

/// Wait the interval the server named, and lengthen it whenever it says so.
fn poll(cloud: &Cloud, start: &penv_cloud::DeviceStart) -> Result<penv_cloud::Grant, CliError> {
    let mut interval = start.interval.max(1);
    let deadline = cloud.now + start.expires_in;
    loop {
        std::thread::sleep(Duration::from_secs(interval));
        match cloud
            .api
            .device_poll(&start.device_code)
            .map_err(|e| refuse(e, None))?
        {
            DevicePoll::Granted(grant) => return Ok(grant),
            DevicePoll::Pending => {}
            DevicePoll::SlowDown => interval += 5,
            DevicePoll::Denied => {
                return Err(CliError::new(
                    "denied",
                    "the sign-in was denied in the console.",
                    "Run penv login again and approve the code it shows.",
                )
                .with_exit(Exit::Auth));
            }
            DevicePoll::Expired => return Err(expired()),
        }
        if penv_cloud::SystemClock.now() >= deadline {
            return Err(expired());
        }
    }
}

fn expired() -> CliError {
    CliError::new(
        "expired",
        "the code expired before it was approved.",
        "Run penv login again.",
    )
    .with_exit(Exit::Auth)
}
