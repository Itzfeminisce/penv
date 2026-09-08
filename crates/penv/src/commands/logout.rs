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

    let Some(bearer) = cloud.user()? else {
        return Ok(Report::new(
            json!({ "signedOut": true, "revoked": false, "server": cloud.api.base_url() }),
            out.style().dim("there was no credential on this host."),
        ));
    };

    // A credential the server has already dropped is still one to forget here.
    let revoked = cloud.api.revoke(&bearer).is_ok();
    cloud
        .keychain
        .delete(penv_cloud::keychain::USER)
        .map_err(|e| refuse(e, None))?;

    Ok(Report::new(
        json!({ "signedOut": true, "revoked": revoked, "server": cloud.api.base_url() }),
        out.style().green("signed out"),
    ))
}
