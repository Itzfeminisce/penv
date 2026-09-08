use std::io::IsTerminal;
use std::path::Path;

use serde_json::json;

use crate::agent::detect_here;
use crate::commands::cloud::{Cloud, refuse};
use crate::env::Env;
use crate::error::CliError;
use crate::output::{Output, Report};

/// Bind this host to a machine identity: a new Ed25519 key, its public half
/// sent once, and the counter that makes a copied keychain visible.
pub fn enroll(out: &Output, _cwd: &Path, secret: &str, env: &Env) -> Result<Report, CliError> {
    if secret.is_empty() {
        return Err(CliError::new(
            "no_secret",
            "penv machine enroll needs the one-time secret from the console.",
            "Issue one in the console and pass it: penv machine enroll pce_...",
        ));
    }

    let detection = detect_here(env, std::io::stdout().is_terminal());
    let cloud = Cloud::open(env, &detection)?;
    if !cloud.keychain.usable() {
        return Err(CliError::new(
            "no_keychain",
            "this host has no keychain, so an enrolled key would not survive the process.",
            "Give the host a PENV_TOKEN, or run it where the OS keychain opens.",
        ));
    }

    let enrolled = penv_cloud::credential::enroll(&cloud.api, cloud.keychain.as_ref(), secret)
        .map_err(|e| refuse(e, None))?;

    let style = out.style();
    Ok(Report::new(
        json!({
            "credentialId": enrolled.credential_id,
            "generation": enrolled.generation,
            "server": cloud.api.base_url(),
        }),
        format!(
            "{} {}\n{}",
            style.green("enrolled"),
            enrolled.credential_id,
            style.dim("the key stays in this host's keychain and is never printed")
        ),
    ))
}
