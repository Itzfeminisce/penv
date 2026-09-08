use std::io::IsTerminal;
use std::path::Path;

use serde_json::json;

use crate::agent::detect_here;
use crate::commands::cloud::{Cloud, refuse};
use crate::env::Env;
use crate::error::CliError;
use crate::output::{Output, Report};

/// Revoke the stored credential and forget it. Both halves are idempotent.
pub fn run(out: &Output, _cwd: &Path, env: &Env) -> Result<Report, CliError> {
    let detection = detect_here(env, std::io::stdout().is_terminal());
    let cloud = Cloud::open(env, &detection)?;

    let held = cloud.user()?;
    // A credential the server has already dropped is still one to forget here.
    let revoked = held
        .as_ref()
        .is_some_and(|bearer| cloud.api.revoke(bearer).is_ok());

    // The cache was sealed against that credential, so it goes with it.
    cloud.forget_cache();
    for item in [penv_cloud::keychain::USER, penv_cloud::keychain::CACHE_KEY] {
        cloud.keychain.delete(item).map_err(|e| refuse(e, None))?;
    }

    if held.is_none() {
        return Ok(Report::new(
            json!({ "signedOut": true, "revoked": false, "server": cloud.api.base_url() }),
            out.style().dim("there was no credential on this host."),
        ));
    }

    Ok(Report::new(
        json!({ "signedOut": true, "revoked": revoked, "server": cloud.api.base_url() }),
        out.style().green("signed out"),
    ))
}
