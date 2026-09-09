//! penv.cloud client: credential kinds, encrypted cache, keychain.

pub mod api;
pub mod b64;
pub mod cache;
pub mod clock;
pub mod credential;
pub mod error;
pub mod fetch;
pub mod keychain;

pub use api::{
    AGENT_HEADER, Address, Api, Bearer, Challenge, CloudKey, DEFAULT_BASE_URL, DevicePoll,
    DeviceStart, Enrolment, EnvBody, Fetched, Freshness, Grant, KeypairGrant, Org, Project,
    PutResult, SESSION_HEADER, SetResult, SignedRequest, Stamp, URL_VAR, UnsetResult, User,
    encode_segment, host_name, without_nulls,
};
pub use cache::{Cache, Entry, Resolved, Source, cache_dir, fetch, ttl_for};
pub use clock::{Clock, Fixed, SystemClock, epoch_from_rfc3339};
pub use credential::{
    AwsIam, BoundKeypair, Enrolled, Obtain, Oidc, TOKEN_VAR, Token, present, resolve,
};
pub use error::{ApiError, CloudError, Result};
pub use fetch::sha256_hex;
pub use keychain::{Keychain, Keyring, MemoryKeychain, NoKeychain};
