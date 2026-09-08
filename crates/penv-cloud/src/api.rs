use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ureq::http::{Response, StatusCode};
use ureq::{Body, RequestBuilder};

use crate::error::{ApiError, CloudError, Result};

/// Where the CLI talks to when nothing says otherwise.
pub const DEFAULT_BASE_URL: &str = "https://penv.cloud";
/// The variable that points the CLI somewhere else.
pub const URL_VAR: &str = "PENV_URL";

/// The two headers the audit row is stamped from.
pub const AGENT_HEADER: &str = "X-Penv-Agent";
pub const SESSION_HEADER: &str = "X-Penv-Session";

const TIMEOUT: Duration = Duration::from_secs(30);

/// A bearer credential. Never printed: `Debug` says only that it exists.
#[derive(Clone, PartialEq, Eq)]
pub struct Bearer {
    pub token: String,
    pub expires_at: Option<u64>,
}

impl Bearer {
    pub fn new(token: impl Into<String>) -> Bearer {
        Bearer {
            token: token.into(),
            expires_at: None,
        }
    }

    pub fn until(token: impl Into<String>, expires_at: u64) -> Bearer {
        Bearer {
            token: token.into(),
            expires_at: Some(expires_at),
        }
    }

    /// The prefix says which principal it is; the rest never leaves this struct.
    pub fn principal(&self) -> &'static str {
        if self.token.starts_with("pcu_") {
            "user"
        } else {
            "machine"
        }
    }
}

impl fmt::Debug for Bearer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bearer")
            .field("principal", &self.principal())
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// `{org}/{project}/{environment}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub org: String,
    pub project: String,
    pub environment: String,
}

impl Address {
    pub fn new(org: &str, project: &str, environment: &str) -> Address {
        Address {
            org: org.to_string(),
            project: project.to_string(),
            environment: environment.to_string(),
        }
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.org, self.project, self.environment)
    }
}

/// A time the server sent. It is quoted back, never arithmetic, unless it came
/// as a number of seconds.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Stamp {
    Epoch(u64),
    Text(String),
}

impl Stamp {
    pub fn epoch(&self) -> Option<u64> {
        match self {
            Stamp::Epoch(n) => Some(*n),
            Stamp::Text(_) => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// What the device token route said this time round.
#[derive(Debug, Clone)]
pub enum DevicePoll {
    Pending,
    SlowDown,
    Expired,
    Denied,
    Granted(Grant),
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    pub credential: String,
    #[serde(default)]
    pub expires_at: Option<Stamp>,
    #[serde(default)]
    pub user: Option<User>,
    #[serde(default)]
    pub orgs: Vec<Org>,
}

impl fmt::Debug for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grant")
            .field("user", &self.user)
            .field("orgs", &self.orgs)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct User {
    pub email: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Org {
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub environments: Vec<String>,
}

/// One parameter as the cloud holds it. `kind` and `version` are the server's
/// own, so a write sends only what [`CloudKey::to_write`] carries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudKey {
    #[serde(default)]
    pub path: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

impl CloudKey {
    /// What a write sends: the address, the schema and the value, never the
    /// version or the kind the server owns.
    pub fn to_write(&self) -> Value {
        let mut out = serde_json::Map::new();
        out.insert("path".into(), Value::String(self.path.clone()));
        out.insert("name".into(), Value::String(self.name.clone()));
        if let Some(schema) = &self.schema {
            out.insert("schema".into(), schema.clone());
        }
        if let Some(value) = &self.value {
            out.insert("value".into(), Value::String(value.clone()));
        }
        Value::Object(out)
    }

    /// `path/name`, the address the server skips and prunes by.
    pub fn address(&self) -> String {
        if self.path.is_empty() {
            self.name.clone()
        } else {
            format!("{}/{}", self.path, self.name)
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvBody {
    #[serde(default)]
    pub keys: Vec<CloudKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
}

/// What a conditional GET came back with.
#[derive(Debug, Clone)]
pub enum Fetched {
    NotModified,
    Body { etag: Option<String>, body: EnvBody },
}

/// What a conditional HEAD came back with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    Unchanged,
    Changed(Option<String>),
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PutResult {
    #[serde(default)]
    pub written: u64,
    #[serde(default)]
    pub unchanged: u64,
    #[serde(default)]
    pub pruned: u64,
    #[serde(default)]
    pub etag: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetResult {
    #[serde(default)]
    pub version: u64,
    #[serde(default)]
    pub etag: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UnsetResult {
    #[serde(default)]
    pub etag: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Challenge {
    pub nonce: String,
}

/// A keypair exchange: the credential, and the counter that has to be on disk
/// before the credential is used.
#[derive(Debug, Clone)]
pub struct KeypairGrant {
    pub bearer: Bearer,
    pub generation: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Enrolment {
    pub credential_id: String,
    #[serde(default = "one")]
    pub generation: u64,
}

fn one() -> u64 {
    1
}

/// An STS request already signed, handed to the server to replay.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SignedRequest {
    pub method: String,
    pub url: String,
    pub body: String,
    pub headers: BTreeMap<String, String>,
}

/// The HTTP surface of penv.cloud. One method per route in docs/Cloud-API.md.
pub struct Api {
    http: ureq::Agent,
    base_url: String,
    agent_name: Option<String>,
    session_id: Option<String>,
}

impl Api {
    pub fn new(base_url: &str) -> Result<Api> {
        let config = ureq::Agent::config_builder()
            .max_redirects(0)
            .max_redirects_will_error(true)
            .http_status_as_error(false)
            .user_agent(format!("penv/{}", env!("CARGO_PKG_VERSION")))
            .timeout_global(Some(TIMEOUT))
            .build();
        Ok(Api {
            http: ureq::Agent::new_with_config(config),
            base_url: checked_base_url(base_url)?,
            agent_name: None,
            session_id: None,
        })
    }

    /// `PENV_URL`, else penv.cloud.
    pub fn from_env(env: &BTreeMap<String, String>) -> Result<Api> {
        let raw = env
            .get(URL_VAR)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
            .unwrap_or(DEFAULT_BASE_URL);
        Api::new(raw)
    }

    /// The agent name and session id every request carries for the audit row.
    pub fn stamped(mut self, agent_name: Option<&str>, session_id: Option<&str>) -> Api {
        self.agent_name = agent_name.filter(|v| !v.is_empty()).map(str::to_string);
        self.session_id = session_id.filter(|v| !v.is_empty()).map(str::to_string);
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base_url)
    }

    fn stamp<A>(&self, mut request: RequestBuilder<A>) -> RequestBuilder<A> {
        if let Some(name) = &self.agent_name {
            request = request.header(AGENT_HEADER, name);
        }
        if let Some(id) = &self.session_id {
            request = request.header(SESSION_HEADER, id);
        }
        request
    }

    fn authed<A>(&self, request: RequestBuilder<A>, bearer: &Bearer) -> RequestBuilder<A> {
        self.stamp(request)
            .header("Authorization", format!("Bearer {}", bearer.token))
    }

    // --- device-code login ---------------------------------------------------

    pub fn device_start(&self) -> Result<DeviceStart> {
        let url = self.url("/auth/device");
        let mut response = send(&url, self.stamp(self.http.post(&url)).send_empty())?;
        expect(&mut response, &[StatusCode::CREATED, StatusCode::OK])?;
        read_json(&url, &mut response)
    }

    pub fn device_poll(&self, device_code: &str) -> Result<DevicePoll> {
        let url = self.url("/auth/device/token");
        let mut response = send(
            &url,
            self.stamp(self.http.post(&url))
                .send_json(json!({ "deviceCode": device_code })),
        )?;
        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(DevicePoll::Granted(read_json(&url, &mut response)?));
        }
        let error = refusal(status, &mut response);
        Ok(match error.code.as_str() {
            "authorization_pending" => DevicePoll::Pending,
            "slow_down" => DevicePoll::SlowDown,
            "expired" => DevicePoll::Expired,
            "denied" => DevicePoll::Denied,
            _ => return Err(error.into()),
        })
    }

    pub fn revoke(&self, bearer: &Bearer) -> Result<()> {
        let url = self.url("/auth/revoke");
        let mut response = send(&url, self.authed(self.http.post(&url), bearer).send_empty())?;
        expect(&mut response, &[StatusCode::OK, StatusCode::NO_CONTENT])?;
        Ok(())
    }

    // --- environments --------------------------------------------------------

    pub fn env_head(&self, bearer: &Bearer, at: &Address, etag: Option<&str>) -> Result<Freshness> {
        let url = self.url(&format!("/envs/{at}"));
        let mut request = self.authed(self.http.head(&url), bearer);
        if let Some(etag) = etag {
            request = request.header("If-None-Match", etag);
        }
        let mut response = send(&url, request.call())?;
        match response.status().as_u16() {
            304 => Ok(Freshness::Unchanged),
            200 => Ok(Freshness::Changed(etag_of(&response))),
            status => Err(refusal(status, &mut response).into()),
        }
    }

    pub fn env_get(
        &self,
        bearer: &Bearer,
        at: &Address,
        etag: Option<&str>,
        values: bool,
    ) -> Result<Fetched> {
        let url = self.url(&format!("/envs/{at}"));
        let mut request = self.authed(self.http.get(&url), bearer);
        if !values {
            request = request.query("values", "false");
        }
        if let Some(etag) = etag {
            request = request.header("If-None-Match", etag);
        }
        let mut response = send(&url, request.call())?;
        match response.status().as_u16() {
            304 => Ok(Fetched::NotModified),
            200 => Ok(Fetched::Body {
                etag: etag_of(&response),
                body: read_json(&url, &mut response)?,
            }),
            status => Err(refusal(status, &mut response).into()),
        }
    }

    pub fn env_put(
        &self,
        bearer: &Bearer,
        at: &Address,
        keys: &[CloudKey],
        prune: bool,
    ) -> Result<PutResult> {
        let url = self.url(&format!("/envs/{at}"));
        let mut response = send(
            &url,
            self.authed(self.http.put(&url), bearer).send_json(json!({
                "keys": keys.iter().map(CloudKey::to_write).collect::<Vec<_>>(),
                "prune": prune,
            })),
        )?;
        expect(&mut response, &[StatusCode::OK])?;
        read_json(&url, &mut response)
    }

    pub fn key_set(&self, bearer: &Bearer, at: &Address, key: &CloudKey) -> Result<SetResult> {
        let url = self.url(&format!("/envs/{at}/keys/{}", key.address()));
        let mut body = key.to_write();
        if let Some(object) = body.as_object_mut() {
            object.remove("path");
            object.remove("name");
        }
        let mut response = send(
            &url,
            self.authed(self.http.patch(&url), bearer).send_json(body),
        )?;
        expect(&mut response, &[StatusCode::OK])?;
        read_json(&url, &mut response)
    }

    pub fn key_unset(&self, bearer: &Bearer, at: &Address, key: &str) -> Result<UnsetResult> {
        let url = self.url(&format!("/envs/{at}/keys/{key}"));
        let mut response = send(&url, self.authed(self.http.delete(&url), bearer).call())?;
        expect(&mut response, &[StatusCode::OK])?;
        read_json(&url, &mut response)
    }

    // --- projects ------------------------------------------------------------

    pub fn orgs(&self, bearer: &Bearer) -> Result<Vec<Org>> {
        #[derive(Deserialize)]
        struct Body {
            #[serde(default)]
            orgs: Vec<Org>,
        }
        let url = self.url("/orgs");
        let mut response = send(&url, self.authed(self.http.get(&url), bearer).call())?;
        expect(&mut response, &[StatusCode::OK])?;
        Ok(read_json::<Body>(&url, &mut response)?.orgs)
    }

    pub fn projects(&self, bearer: &Bearer, org: &str) -> Result<Vec<Project>> {
        #[derive(Deserialize)]
        struct Body {
            #[serde(default)]
            projects: Vec<Project>,
        }
        let url = self.url(&format!("/orgs/{org}/projects"));
        let mut response = send(&url, self.authed(self.http.get(&url), bearer).call())?;
        expect(&mut response, &[StatusCode::OK])?;
        Ok(read_json::<Body>(&url, &mut response)?.projects)
    }

    pub fn create_project(
        &self,
        bearer: &Bearer,
        org: &str,
        name: &str,
        environments: &[String],
    ) -> Result<()> {
        let url = self.url(&format!("/orgs/{org}/projects"));
        let mut response = send(
            &url,
            self.authed(self.http.post(&url), bearer)
                .send_json(json!({ "name": name, "environments": environments })),
        )?;
        expect(&mut response, &[StatusCode::CREATED, StatusCode::OK])?;
        Ok(())
    }

    // --- credential exchanges ------------------------------------------------

    pub fn exchange_oidc(&self, token: &str, now: u64) -> Result<Bearer> {
        let url = self.url("/auth/oidc");
        let mut response = send(
            &url,
            self.stamp(self.http.post(&url))
                .send_json(json!({ "token": token })),
        )?;
        expect(&mut response, &[StatusCode::OK, StatusCode::CREATED])?;
        bearer_from(&url, &mut response, now)
    }

    pub fn exchange_aws(&self, signed: &SignedRequest, now: u64) -> Result<Bearer> {
        let url = self.url("/auth/aws");
        let mut response = send(&url, self.stamp(self.http.post(&url)).send_json(signed))?;
        expect(&mut response, &[StatusCode::OK, StatusCode::CREATED])?;
        bearer_from(&url, &mut response, now)
    }

    pub fn keypair_challenge(&self, credential_id: &str) -> Result<Challenge> {
        let url = self.url("/auth/keypair/challenge");
        let mut response = send(
            &url,
            self.stamp(self.http.post(&url))
                .send_json(json!({ "credentialId": credential_id })),
        )?;
        expect(&mut response, &[StatusCode::OK, StatusCode::CREATED])?;
        read_json(&url, &mut response)
    }

    pub fn exchange_keypair(
        &self,
        credential_id: &str,
        nonce: &str,
        generation: u64,
        signature: &str,
        now: u64,
    ) -> Result<KeypairGrant> {
        let url = self.url("/auth/keypair");
        let mut response = send(
            &url,
            self.stamp(self.http.post(&url)).send_json(json!({
                "credentialId": credential_id,
                "nonce": nonce,
                "generation": generation,
                "signature": signature,
            })),
        )?;
        expect(&mut response, &[StatusCode::OK, StatusCode::CREATED])?;
        let body: Value = read_json(&url, &mut response)?;
        Ok(KeypairGrant {
            generation: body
                .get("generation")
                .and_then(Value::as_u64)
                .unwrap_or(generation + 1),
            bearer: bearer_of(&url, &body, now)?,
        })
    }

    pub fn keypair_enroll(&self, secret: &str, public_key: &str) -> Result<Enrolment> {
        let url = self.url("/auth/keypair/enroll");
        let mut response = send(
            &url,
            self.stamp(self.http.post(&url))
                .send_json(json!({ "secret": secret, "publicKey": public_key })),
        )?;
        expect(&mut response, &[StatusCode::OK, StatusCode::CREATED])?;
        read_json(&url, &mut response)
    }

    /// The platform's own OIDC endpoint, which is not penv.cloud. GitHub Actions
    /// mints one token per audience and hands it over from this URL.
    pub fn platform_id_token(
        &self,
        url: &str,
        request_token: &str,
        audience: Option<&str>,
    ) -> Result<String> {
        #[derive(Deserialize)]
        struct Body {
            value: String,
        }
        let mut request = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {request_token}"))
            .header("Accept", "application/json");
        if let Some(audience) = audience {
            request = request.query("audience", audience);
        }
        let mut response = send(url, request.call())?;
        expect(&mut response, &[StatusCode::OK])?;
        Ok(read_json::<Body>(url, &mut response)?.value)
    }
}

/// https everywhere but the loopback the tests and a local server run on.
pub fn checked_base_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim_end_matches('/');
    let (scheme, rest) = trimmed
        .split_once("://")
        .ok_or_else(|| CloudError::Url(format!("{raw} is not an http or https URL")))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.split(':').next().unwrap_or_default();
    match scheme {
        "https" => Ok(trimmed.to_string()),
        "http" if matches!(host, "127.0.0.1" | "localhost") => Ok(trimmed.to_string()),
        "http" => Err(CloudError::Url(format!(
            "{raw} is plain http, and a credential only travels over https"
        ))),
        other => Err(CloudError::Url(format!(
            "{other} is not a scheme penv speaks"
        ))),
    }
}

fn send(
    url: &str,
    sent: std::result::Result<Response<Body>, ureq::Error>,
) -> Result<Response<Body>> {
    sent.map_err(|e| match e {
        ureq::Error::TooManyRedirects | ureq::Error::RedirectFailed => CloudError::Redirected {
            url: url.to_string(),
        },
        other => CloudError::Offline {
            url: url.to_string(),
            reason: other.to_string(),
        },
    })
}

fn expect(response: &mut Response<Body>, ok: &[StatusCode]) -> Result<()> {
    let status = response.status();
    if ok.contains(&status) {
        return Ok(());
    }
    Err(refusal(status.as_u16(), response).into())
}

/// The body's `error` code, the status, and how long the server asked us to wait.
fn refusal(status: u16, response: &mut Response<Body>) -> ApiError {
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    let body: Option<Value> = response.body_mut().read_json().ok();
    let code = body
        .as_ref()
        .and_then(|b| b.get("error"))
        .and_then(Value::as_str)
        .unwrap_or(match status {
            401 => "unauthorized",
            403 => "forbidden",
            404 => "not_found",
            409 => "conflict",
            429 => "rate_limited",
            _ => "error",
        })
        .to_string();
    let retry_after = retry_after.or_else(|| {
        body.as_ref()
            .and_then(|b| b.get("retryAfter"))
            .and_then(Value::as_u64)
    });
    ApiError::new(status, code).after(retry_after)
}

fn read_json<T: DeserializeOwned>(url: &str, response: &mut Response<Body>) -> Result<T> {
    response
        .body_mut()
        .read_json()
        .map_err(|e| CloudError::Unreadable {
            url: url.to_string(),
            reason: e.to_string(),
        })
}

fn etag_of(response: &Response<Body>) -> Option<String> {
    response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn bearer_from(url: &str, response: &mut Response<Body>, now: u64) -> Result<Bearer> {
    let body: Value = read_json(url, response)?;
    bearer_of(url, &body, now)
}

/// The exchanges answer with the same credential shape the device route uses.
fn bearer_of(url: &str, body: &Value, now: u64) -> Result<Bearer> {
    let token = body
        .get("credential")
        .or_else(|| body.get("token"))
        .and_then(Value::as_str)
        .ok_or_else(|| CloudError::Unreadable {
            url: url.to_string(),
            reason: "no credential in the answer".into(),
        })?;
    let expires_at = body
        .get("expiresIn")
        .and_then(Value::as_u64)
        .map(|seconds| now + seconds)
        .or_else(|| body.get("expiresAt").and_then(Value::as_u64));
    Ok(Bearer {
        token: token.to_string(),
        expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_is_required_everywhere_but_the_loopback() {
        assert_eq!(
            checked_base_url("https://penv.cloud/").unwrap(),
            "https://penv.cloud"
        );
        assert_eq!(
            checked_base_url("http://127.0.0.1:8787").unwrap(),
            "http://127.0.0.1:8787"
        );
        assert!(checked_base_url("http://localhost:1234").is_ok());
        assert!(checked_base_url("http://penv.cloud").is_err());
        assert!(checked_base_url("ftp://penv.cloud").is_err());
        assert!(checked_base_url("penv.cloud").is_err());
    }

    #[test]
    fn a_bearer_never_prints_itself() {
        let shown = format!("{:?}", Bearer::new("pcu_FAKE_NEVER_PRINTED"));
        assert!(!shown.contains("FAKE"), "{shown}");
        assert!(shown.contains("user"), "{shown}");
    }

    #[test]
    fn a_write_never_sends_back_what_only_the_server_owns() {
        let key = CloudKey {
            path: String::new(),
            name: "PORT".into(),
            kind: Some("static".into()),
            version: Some(3),
            schema: Some(json!({ "type": "port" })),
            value: Some("3000".into()),
        };
        let sent = key.to_write();
        assert_eq!(sent.get("kind"), None);
        assert_eq!(sent.get("version"), None);
        assert_eq!(sent["name"], "PORT");
        assert_eq!(sent["value"], "3000");
    }

    #[test]
    fn an_address_is_the_path_the_routes_use() {
        let at = Address::new("acme", "api-gateway", "development");
        assert_eq!(at.to_string(), "acme/api-gateway/development");
    }
}
