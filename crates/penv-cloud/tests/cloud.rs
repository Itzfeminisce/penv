//! The client against an in-process server: every credential kind, the cache
//! rules of design section 5, and the headers the audit row is built from.

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use common::Mock;
use penv_cloud::api::{Address, Api, Bearer, CloudKey, DevicePoll, Fetched, Freshness};
use penv_cloud::cache::{self, Cache, Entry, Source};
use penv_cloud::credential::{self, AwsIam, BoundKeypair, Enrolled, Obtain, Oidc, Token};
use penv_cloud::error::{CloudError, Result};
use penv_cloud::keychain::{self, Keychain, MemoryKeychain, NoKeychain};
use penv_cloud::{EnvBody, b64};
use serde_json::json;

const ENVS: &str = "/api/v1/envs/acme/api/development";
const NOW: u64 = 1_757_000_000;

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn address() -> Address {
    Address::new("acme", "api", "development")
}

fn api(mock: &Mock) -> Api {
    Api::new(&mock.url())
        .unwrap()
        .stamped(Some("claude"), Some("sess-7"))
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "penv-cloud-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

fn body(keys: &[(&str, &str)]) -> String {
    let keys: Vec<_> = keys
        .iter()
        .map(|(name, value)| json!({ "path": "", "name": name, "kind": "static", "version": 1, "value": value }))
        .collect();
    json!({ "keys": keys }).to_string()
}

// --- credential kinds -------------------------------------------------------

#[test]
fn a_token_is_its_own_bearer_and_asks_nobody() {
    let mock = Mock::new();
    let bearer = Token::new("pck_FAKE").obtain(&api(&mock), NOW).unwrap();
    assert_eq!(bearer.token, "pck_FAKE");
    assert!(mock.requests().is_empty(), "a token exchanges nothing");
}

#[test]
fn every_request_carries_the_agent_and_the_session() {
    let mock = Mock::new();
    mock.on("GET", ENVS, 200, &body(&[("PORT", "3000")]));
    api(&mock)
        .env_get(&Bearer::new("pck_FAKE"), &address(), None, true)
        .unwrap();

    let request = mock.last("GET", ENVS);
    assert_eq!(request.header("x-penv-agent"), Some("claude"));
    assert_eq!(request.header("x-penv-session"), Some("sess-7"));
    assert_eq!(request.header("authorization"), Some("Bearer pck_FAKE"));
}

#[test]
fn a_held_oidc_token_is_exchanged_for_a_short_lived_credential() {
    let mock = Mock::new();
    mock.on(
        "POST",
        "/api/v1/auth/oidc",
        200,
        &json!({ "credential": "pck_EXCHANGED", "expiresIn": 900 }).to_string(),
    );
    let oidc = Oidc::from_env(&env(&[("PENV_OIDC_TOKEN", "jwt_FAKE")]), Some("acme")).unwrap();
    let bearer = oidc.obtain(&api(&mock), NOW).unwrap();

    assert_eq!(bearer.token, "pck_EXCHANGED");
    assert_eq!(bearer.expires_at, Some(NOW + 900));
    assert_eq!(
        mock.last("POST", "/api/v1/auth/oidc").json(),
        json!({ "token": "jwt_FAKE" })
    );
}

#[test]
fn github_actions_fetches_its_token_for_the_org_before_exchanging_it() {
    let mock = Mock::new();
    mock.on(
        "GET",
        "/token",
        200,
        &json!({ "value": "jwt_FROM_GITHUB" }).to_string(),
    );
    mock.on(
        "POST",
        "/api/v1/auth/oidc",
        200,
        &json!({ "credential": "pck_EXCHANGED" }).to_string(),
    );
    let oidc = Oidc::from_env(
        &env(&[
            (
                "ACTIONS_ID_TOKEN_REQUEST_URL",
                &format!("{}/token", mock.url()),
            ),
            ("ACTIONS_ID_TOKEN_REQUEST_TOKEN", "request_FAKE"),
        ]),
        Some("acme"),
    )
    .unwrap();
    let bearer = oidc.obtain(&api(&mock), NOW).unwrap();

    assert_eq!(bearer.token, "pck_EXCHANGED");
    let minted = mock.last("GET", "/token");
    assert!(minted.target.contains("audience=acme"), "{}", minted.target);
    assert_eq!(minted.header("authorization"), Some("Bearer request_FAKE"));
    assert_eq!(
        mock.last("POST", "/api/v1/auth/oidc").json()["token"],
        "jwt_FROM_GITHUB"
    );
}

#[test]
fn aws_posts_a_signed_sts_request_and_never_the_secret_key() {
    let mock = Mock::new();
    mock.on(
        "POST",
        "/api/v1/auth/aws",
        200,
        &json!({ "credential": "pck_EXCHANGED" }).to_string(),
    );
    let aws = AwsIam::from_env(&env(&[
        ("AWS_ACCESS_KEY_ID", "AKIAFAKE"),
        ("AWS_SECRET_ACCESS_KEY", "secretFAKE"),
        ("AWS_REGION", "eu-west-1"),
    ]))
    .unwrap();
    assert_eq!(aws.obtain(&api(&mock), NOW).unwrap().token, "pck_EXCHANGED");

    let sent = mock.last("POST", "/api/v1/auth/aws");
    assert!(
        !sent.body.contains("secretFAKE"),
        "the secret key never travels"
    );
    let json = sent.json();
    assert_eq!(json["method"], "POST");
    assert_eq!(json["url"], "https://sts.eu-west-1.amazonaws.com/");
    assert_eq!(json["body"], "Action=GetCallerIdentity&Version=2011-06-15");
    assert!(
        json["headers"]["authorization"]
            .as_str()
            .unwrap()
            .starts_with("AWS4-HMAC-SHA256")
    );
}

fn enrolled(store: &MemoryKeychain, generation: u64) -> Enrolled {
    let enrolled = Enrolled {
        credential_id: "pcm_FAKE".into(),
        secret: b64::encode(&[3u8; 32]),
        generation,
    };
    store
        .set(
            keychain::KEYPAIR,
            &serde_json::to_string(&enrolled).unwrap(),
        )
        .unwrap();
    enrolled
}

fn challenge_and_grant(mock: &Mock, generation: u64) {
    mock.on(
        "POST",
        "/api/v1/auth/keypair/challenge",
        200,
        &json!({ "nonce": "nonce-1" }).to_string(),
    );
    mock.on(
        "POST",
        "/api/v1/auth/keypair",
        200,
        &json!({ "credential": "pck_EXCHANGED", "generation": generation }).to_string(),
    );
}

#[test]
fn a_keypair_signs_the_four_lines_and_banks_the_generation_before_it_is_used() {
    let mock = Mock::new();
    challenge_and_grant(&mock, 8);
    let store = MemoryKeychain::new();
    enrolled(&store, 7);

    let bearer = BoundKeypair::from_keychain(&store)
        .unwrap()
        .expect("an enrolled key")
        .obtain(&api(&mock), NOW)
        .unwrap();
    assert_eq!(bearer.token, "pck_EXCHANGED");

    let sent = mock.last("POST", "/api/v1/auth/keypair").json();
    assert_eq!(sent["credentialId"], "pcm_FAKE");
    assert_eq!(sent["nonce"], "nonce-1");
    assert_eq!(
        sent["generation"], 7,
        "the counter it held is what it signs"
    );
    let signature = b64::decode(sent["signature"].as_str().unwrap()).expect("base64");
    assert_eq!(signature.len(), 64);

    let stored: serde_json::Value =
        serde_json::from_str(&store.get(keychain::KEYPAIR).unwrap().unwrap()).unwrap();
    assert_eq!(stored["generation"], 8, "the new counter is on disk");
}

/// A store that will not take the new counter. The credential must not be
/// handed back, or the next run signs a generation the server reads as a clone.
struct Brittle(MemoryKeychain);

impl Keychain for Brittle {
    fn get(&self, item: &str) -> Result<Option<String>> {
        self.0.get(item)
    }
    fn set(&self, _item: &str, _value: &str) -> Result<()> {
        Err(CloudError::Keychain("the store is read only".into()))
    }
    fn delete(&self, item: &str) -> Result<()> {
        self.0.delete(item)
    }
}

#[test]
fn a_generation_that_cannot_be_banked_refuses_the_credential() {
    let mock = Mock::new();
    challenge_and_grant(&mock, 8);
    let inner = MemoryKeychain::new();
    let enrolled = enrolled(&inner, 7);
    let store = Brittle(inner);

    let error = BoundKeypair::new(&store, enrolled)
        .obtain(&api(&mock), NOW)
        .unwrap_err();
    assert!(matches!(error, CloudError::Keychain(_)), "{error:?}");
}

#[test]
fn a_cloned_keychain_is_named_by_the_server_and_kept() {
    let mock = Mock::new();
    mock.on(
        "POST",
        "/api/v1/auth/keypair/challenge",
        200,
        &json!({ "nonce": "nonce-1" }).to_string(),
    );
    mock.on(
        "POST",
        "/api/v1/auth/keypair",
        409,
        &json!({ "error": "cloned" }).to_string(),
    );
    let store = MemoryKeychain::new();
    enrolled(&store, 7);

    let error = BoundKeypair::from_keychain(&store)
        .unwrap()
        .unwrap()
        .obtain(&api(&mock), NOW)
        .unwrap_err();
    assert_eq!(error.status(), Some(409));
    assert!(error.is("cloned"), "{error:?}");
}

#[test]
fn enrolling_stores_the_key_the_public_half_was_sent_for() {
    let mock = Mock::new();
    mock.on(
        "POST",
        "/api/v1/auth/keypair/enroll",
        201,
        &json!({ "credentialId": "pcm_NEW", "generation": 1 }).to_string(),
    );
    let store = MemoryKeychain::new();
    let enrolled = credential::enroll(&api(&mock), &store, "pce_secret_FAKE").unwrap();

    assert_eq!(enrolled.credential_id, "pcm_NEW");
    assert_eq!(enrolled.generation, 1);
    let sent = mock.last("POST", "/api/v1/auth/keypair/enroll").json();
    assert_eq!(sent["secret"], "pce_secret_FAKE");
    let der = b64::decode(sent["publicKey"].as_str().unwrap()).expect("base64 DER");
    assert_eq!(der.len(), 44, "SPKI DER for Ed25519");
    assert!(store.get(keychain::KEYPAIR).unwrap().is_some());
}

#[test]
fn resolution_walks_the_order_the_design_fixes() {
    let store = MemoryKeychain::new();
    assert!(matches!(
        credential::resolve(&env(&[]), &store, None),
        Err(CloudError::NoCredential)
    ));

    let aws = env(&[
        ("AWS_ACCESS_KEY_ID", "AKIAFAKE"),
        ("AWS_SECRET_ACCESS_KEY", "secretFAKE"),
    ]);
    assert!(credential::resolve(&aws, &store, None).is_ok());

    store.set(keychain::USER, "pcu_FAKE").unwrap();
    assert!(credential::present(&env(&[]), &store));
    assert!(!credential::present(&env(&[]), &NoKeychain));
}

// --- device-code login ------------------------------------------------------

#[test]
fn the_device_flow_waits_slows_down_and_then_lands() {
    let mock = Mock::new();
    mock.on(
        "POST",
        "/api/v1/auth/device",
        201,
        &json!({
            "deviceCode": "dc_FAKE",
            "userCode": "WXYZ-1234",
            "verificationUri": "https://penv.cloud/device",
            "expiresIn": 600,
            "interval": 5
        })
        .to_string(),
    );
    mock.on(
        "POST",
        "/api/v1/auth/device/token",
        428,
        &json!({ "error": "authorization_pending" }).to_string(),
    );
    mock.on(
        "POST",
        "/api/v1/auth/device/token",
        429,
        &json!({ "error": "slow_down" }).to_string(),
    );
    mock.on(
        "POST",
        "/api/v1/auth/device/token",
        201,
        &json!({
            "credential": "pcu_FAKE",
            "user": { "email": "dev@example.com" },
            "orgs": [{ "slug": "acme", "name": "Acme" }]
        })
        .to_string(),
    );

    let api = api(&mock);
    let start = api.device_start().unwrap();
    assert_eq!(start.user_code, "WXYZ-1234");
    assert_eq!(start.interval, 5);

    assert!(matches!(
        api.device_poll(&start.device_code).unwrap(),
        DevicePoll::Pending
    ));
    assert!(matches!(
        api.device_poll(&start.device_code).unwrap(),
        DevicePoll::SlowDown
    ));
    let DevicePoll::Granted(grant) = api.device_poll(&start.device_code).unwrap() else {
        panic!("the third poll lands");
    };
    assert_eq!(grant.credential, "pcu_FAKE");
    assert_eq!(grant.user.unwrap().email, "dev@example.com");
    assert_eq!(grant.orgs[0].slug, "acme");
    assert_eq!(
        mock.last("POST", "/api/v1/auth/device/token").json()["deviceCode"],
        "dc_FAKE"
    );
}

#[test]
fn a_denied_or_expired_device_code_is_an_answer_not_an_error() {
    for (status, code, expected) in [(410u16, "expired", "expired"), (403, "denied", "denied")] {
        let mock = Mock::new();
        mock.on(
            "POST",
            "/api/v1/auth/device/token",
            status,
            &json!({ "error": code }).to_string(),
        );
        let answer = api(&mock).device_poll("dc_FAKE").unwrap();
        match (expected, answer) {
            ("expired", DevicePoll::Expired) | ("denied", DevicePoll::Denied) => {}
            (_, other) => panic!("{code} came back as {other:?}"),
        }
    }
}

// --- environments -----------------------------------------------------------

#[test]
fn an_environment_this_identity_may_not_read_comes_back_as_forbidden() {
    let mock = Mock::new();
    mock.on(
        "GET",
        "/api/v1/envs/acme/api/production",
        403,
        &json!({ "error": "forbidden" }).to_string(),
    );
    let error = api(&mock)
        .env_get(
            &Bearer::new("pcu_FAKE"),
            &Address::new("acme", "api", "production"),
            None,
            true,
        )
        .unwrap_err();
    assert_eq!(error.status(), Some(403));
    assert!(error.is("forbidden"), "{error:?}");
}

#[test]
fn a_rate_limit_carries_the_wait_the_server_asked_for() {
    let mock = Mock::new();
    mock.on_with(
        "GET",
        ENVS,
        429,
        &json!({ "error": "rate_limited" }).to_string(),
        &[("Retry-After", "12")],
    );
    let error = api(&mock)
        .env_get(&Bearer::new("pck_FAKE"), &address(), None, true)
        .unwrap_err();
    let CloudError::Api(api_error) = error else {
        panic!("a refusal");
    };
    assert_eq!(api_error.retry_after, Some(12));
}

#[test]
fn push_and_the_per_key_writes_speak_the_documented_shapes() {
    let mock = Mock::new();
    mock.on(
        "PUT",
        ENVS,
        200,
        &json!({ "written": 2, "unchanged": 0, "pruned": 0, "etag": "\"abc\"" }).to_string(),
    );
    mock.on(
        "PATCH",
        &format!("{ENVS}/keys/PORT"),
        200,
        &json!({ "version": 4, "etag": "\"def\"" }).to_string(),
    );
    mock.on(
        "DELETE",
        &format!("{ENVS}/keys/PORT"),
        200,
        &json!({ "etag": "\"ghi\"" }).to_string(),
    );

    let api = api(&mock);
    let bearer = Bearer::new("pcu_FAKE");
    let keys = vec![CloudKey {
        name: "PORT".into(),
        schema: Some(json!({ "type": { "name": "port" } })),
        value: Some("3000".into()),
        ..CloudKey::default()
    }];
    let put = api.env_put(&bearer, &address(), &keys, false).unwrap();
    assert_eq!(put.written, 2);
    assert_eq!(put.etag, "\"abc\"");
    let sent = mock.last("PUT", ENVS).json();
    assert_eq!(sent["prune"], false);
    assert_eq!(sent["keys"][0]["name"], "PORT");
    assert_eq!(sent["keys"][0]["value"], "3000");

    assert_eq!(
        api.key_set(&bearer, &address(), &keys[0]).unwrap().version,
        4
    );
    assert_eq!(
        api.key_unset(&bearer, &address(), "PORT").unwrap().etag,
        "\"ghi\""
    );
}

#[test]
fn orgs_and_projects_are_read_and_created() {
    let mock = Mock::new();
    mock.on(
        "GET",
        "/api/v1/orgs",
        200,
        &json!({ "orgs": [{ "slug": "acme", "name": "Acme" }] }).to_string(),
    );
    mock.on(
        "GET",
        "/api/v1/orgs/acme/projects",
        200,
        &json!({ "projects": [{ "slug": "api", "name": "api", "environments": ["development"] }] })
            .to_string(),
    );
    mock.on("POST", "/api/v1/orgs/acme/projects", 201, "{}");

    let api = api(&mock);
    let bearer = Bearer::new("pcu_FAKE");
    assert_eq!(api.orgs(&bearer).unwrap()[0].slug, "acme");
    assert_eq!(
        api.projects(&bearer, "acme").unwrap()[0].environments,
        ["development"]
    );
    api.create_project(&bearer, "acme", "api", &["development".to_string()])
        .unwrap();
    let sent = mock.last("POST", "/api/v1/orgs/acme/projects").json();
    assert_eq!(sent["name"], "api");
    assert_eq!(sent["environments"][0], "development");
}

// --- cache ------------------------------------------------------------------

fn entry(at: u64) -> Entry {
    Entry {
        etag: Some("\"one\"".into()),
        fetched_at: at,
        body: serde_json::from_str(&body(&[("PORT", "3000")])).unwrap(),
    }
}

#[test]
fn a_cache_file_round_trips_and_belongs_to_one_address_only() {
    let dir = scratch();
    let store = MemoryKeychain::new();
    let cache = Cache::open(&dir, "https://penv.cloud", &address(), &store)
        .unwrap()
        .expect("a keychain means a cache");
    cache.write(&entry(NOW)).unwrap();

    assert_eq!(cache.read().unwrap(), entry(NOW));
    let raw = std::fs::read(cache.path()).unwrap();
    assert!(
        !String::from_utf8_lossy(&raw).contains("PORT"),
        "it is encrypted"
    );

    let elsewhere = Cache::open(
        &dir,
        "https://penv.cloud",
        &Address::new("acme", "api", "production"),
        &store,
    )
    .unwrap()
    .unwrap();
    std::fs::copy(cache.path(), elsewhere.path()).unwrap();
    assert!(elsewhere.read().is_none(), "the AAD binds the address");

    assert!(
        Cache::open(&dir, "https://penv.cloud", &address(), &NoKeychain)
            .unwrap()
            .is_none(),
        "no keychain, no cache"
    );
}

#[test]
fn a_fresh_development_cache_asks_the_server_nothing() {
    let dir = scratch();
    let store = MemoryKeychain::new();
    let mock = Mock::new();
    let cache = Cache::open(&dir, &mock.url(), &address(), &store)
        .unwrap()
        .unwrap();
    cache.write(&entry(NOW)).unwrap();

    let resolved = cache
        .revalidate(&api(&mock), &Bearer::new("pck_FAKE"), NOW + 30)
        .unwrap();
    assert_eq!(resolved.source, Source::Cache);
    assert!(
        mock.requests().is_empty(),
        "inside the TTL nothing is asked"
    );
    assert_eq!(cache::ttl_for("development"), 60);
}

#[test]
fn a_stale_cache_revalidates_with_the_etag_and_only_refetches_when_it_changed() {
    let dir = scratch();
    let store = MemoryKeychain::new();
    let mock = Mock::new();
    mock.on("HEAD", ENVS, 304, "");
    let cache = Cache::open(&dir, &mock.url(), &address(), &store)
        .unwrap()
        .unwrap();
    cache.write(&entry(NOW)).unwrap();

    let api = api(&mock);
    let bearer = Bearer::new("pck_FAKE");
    let resolved = cache.revalidate(&api, &bearer, NOW + 90).unwrap();
    assert_eq!(resolved.body.keys[0].value.as_deref(), Some("3000"));
    assert_eq!(
        mock.last("HEAD", ENVS).header("if-none-match"),
        Some("\"one\"")
    );
    assert_eq!(
        cache.read().unwrap().fetched_at,
        NOW + 90,
        "the clock moved on"
    );
    assert!(mock.hits("GET", ENVS).is_empty(), "304 needs no body");
}

#[test]
fn a_changed_environment_is_fetched_again_and_rewritten() {
    let dir = scratch();
    let store = MemoryKeychain::new();
    let mock = Mock::new();
    mock.on_with("HEAD", ENVS, 200, "", &[("ETag", "\"two\"")]);
    mock.on_with(
        "GET",
        ENVS,
        200,
        &body(&[("PORT", "4000")]),
        &[("ETag", "\"two\"")],
    );
    let cache = Cache::open(&dir, &mock.url(), &address(), &store)
        .unwrap()
        .unwrap();
    cache.write(&entry(NOW)).unwrap();

    let resolved = cache
        .revalidate(&api(&mock), &Bearer::new("pck_FAKE"), NOW + 90)
        .unwrap();
    assert_eq!(resolved.source, Source::Server);
    assert_eq!(resolved.body.keys[0].value.as_deref(), Some("4000"));
    assert_eq!(resolved.etag.as_deref(), Some("\"two\""));
    assert_eq!(cache.read().unwrap().etag.as_deref(), Some("\"two\""));
}

#[test]
fn offline_development_runs_on_the_cache_and_says_so_once_a_day() {
    let dir = scratch();
    let store = MemoryKeychain::new();
    let closed = Mock::closed_url();
    let api = Api::new(&closed).unwrap();
    let cache = Cache::open(&dir, &closed, &address(), &store)
        .unwrap()
        .unwrap();
    cache.write(&entry(NOW)).unwrap();

    let bearer = Bearer::new("pck_FAKE");
    let first = cache.revalidate(&api, &bearer, NOW + 90).unwrap();
    assert_eq!(first.source, Source::Cache);
    assert!(first.offline_warning, "the first offline run says so");

    let second = cache.revalidate(&api, &bearer, NOW + 180).unwrap();
    assert!(!second.offline_warning, "and then stays quiet");

    let tomorrow = cache.revalidate(&api, &bearer, NOW + 90 + 86_400).unwrap();
    assert!(tomorrow.offline_warning, "and says it again the next day");
}

#[test]
fn offline_fails_closed_everywhere_but_development() {
    let dir = scratch();
    let store = MemoryKeychain::new();
    let closed = Mock::closed_url();
    let at = Address::new("acme", "api", "production");
    let cache = Cache::open(&dir, &closed, &at, &store).unwrap().unwrap();
    cache.write(&entry(NOW)).unwrap();

    let error = cache
        .revalidate(&Api::new(&closed).unwrap(), &Bearer::new("pck_FAKE"), NOW)
        .unwrap_err();
    assert!(error.is_offline(), "{error:?}");
    assert_eq!(cache::ttl_for("production"), 0);
}

#[test]
fn a_host_with_no_cache_reads_the_server_every_time() {
    let mock = Mock::new();
    mock.on("GET", ENVS, 200, &body(&[("PORT", "3000")]));
    let resolved =
        cache::fetch(&api(&mock), &Bearer::new("pck_FAKE"), &address(), None, NOW).unwrap();
    assert_eq!(resolved.source, Source::Server);
    assert_eq!(resolved.body.keys.len(), 1);
}

#[test]
fn the_conditional_reads_answer_unchanged_when_the_server_says_so() {
    let mock = Mock::new();
    mock.on("HEAD", ENVS, 304, "");
    mock.on("GET", ENVS, 304, "");
    let api = api(&mock);
    let bearer = Bearer::new("pck_FAKE");

    assert_eq!(
        api.env_head(&bearer, &address(), Some("\"one\"")).unwrap(),
        Freshness::Unchanged
    );
    assert!(matches!(
        api.env_get(&bearer, &address(), Some("\"one\""), true)
            .unwrap(),
        Fetched::NotModified
    ));
}

#[test]
fn a_schema_only_read_says_so_in_the_query() {
    let mock = Mock::new();
    mock.on("GET", ENVS, 200, &json!({ "keys": [] }).to_string());
    api(&mock)
        .env_get(&Bearer::new("pcu_FAKE"), &address(), None, false)
        .unwrap();
    assert!(
        mock.last("GET", ENVS).target.contains("values=false"),
        "{}",
        mock.last("GET", ENVS).target
    );
}

#[test]
fn an_empty_environment_reads_as_an_empty_body() {
    let parsed: EnvBody = serde_json::from_str("{}").unwrap();
    assert!(parsed.keys.is_empty() && parsed.skipped.is_empty());
}
