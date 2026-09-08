use std::fmt;

use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};

use crate::api::{Api, Bearer};
use crate::b64;
use crate::credential::Obtain;
use crate::error::{CloudError, Result};
use crate::keychain::{self, Keychain};

/// The first line of what the key signs. Changing it is a new protocol version.
pub const MESSAGE_PREFIX: &str = "penv-cloud:keypair:v1";

/// SubjectPublicKeyInfo for Ed25519 is a fixed header and the 32 key bytes.
const SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

/// What the keychain holds for an enrolled host.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Enrolled {
    pub credential_id: String,
    /// The 32 secret bytes, base64. Never printed.
    pub secret: String,
    pub generation: u64,
}

impl fmt::Debug for Enrolled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Enrolled")
            .field("credential_id", &self.credential_id)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl Enrolled {
    fn key(&self) -> Result<SigningKey> {
        let bytes = b64::decode(&self.secret)
            .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
            .ok_or_else(|| CloudError::Credential("the enrolled key is not 32 bytes".into()))?;
        Ok(SigningKey::from_bytes(&bytes))
    }
}

/// An Ed25519 key bound to one machine identity, with a counter that makes a
/// copied keychain visible to the server.
pub struct BoundKeypair<'a> {
    store: &'a dyn Keychain,
    enrolled: Enrolled,
}

impl<'a> BoundKeypair<'a> {
    pub fn new(store: &'a dyn Keychain, enrolled: Enrolled) -> BoundKeypair<'a> {
        BoundKeypair { store, enrolled }
    }

    pub fn from_keychain(store: &'a dyn Keychain) -> Result<Option<BoundKeypair<'a>>> {
        let Some(stored) = store.get(keychain::KEYPAIR)? else {
            return Ok(None);
        };
        let enrolled: Enrolled = serde_json::from_str(&stored)
            .map_err(|e| CloudError::Credential(format!("the enrolled key is unreadable: {e}")))?;
        Ok(Some(BoundKeypair { store, enrolled }))
    }

    pub fn credential_id(&self) -> &str {
        &self.enrolled.credential_id
    }
}

impl fmt::Debug for BoundKeypair<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundKeypair")
            .field("credential_id", &self.enrolled.credential_id)
            .finish_non_exhaustive()
    }
}

impl Obtain for BoundKeypair<'_> {
    fn obtain(&self, api: &Api, now: u64) -> Result<Bearer> {
        let challenge = api.keypair_challenge(&self.enrolled.credential_id)?;
        let signature = sign_message(
            &self.enrolled.key()?,
            &message(
                &self.enrolled.credential_id,
                &challenge.nonce,
                self.enrolled.generation,
            ),
        );
        let grant = api.exchange_keypair(
            &self.enrolled.credential_id,
            &challenge.nonce,
            self.enrolled.generation,
            &signature,
            now,
        )?;
        // The counter goes to disk before the credential is used: a crash costs a
        // re-enrolment, never a generation the server will read as a clone.
        remember(
            self.store,
            keychain::KEYPAIR,
            &Enrolled {
                generation: grant.generation,
                ..self.enrolled.clone()
            },
        )?;
        Ok(grant.bearer)
    }
}

/// Bind this host to a machine identity from a one-time secret. The private key
/// is on disk before the public half is sent, so a lost answer leaves a pending
/// item to clear rather than a key the server knows and this host does not.
pub fn enroll(api: &Api, store: &dyn Keychain, secret: &str) -> Result<Enrolled> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|e| CloudError::Credential(format!("no key could be generated: {e}")))?;
    let key = SigningKey::from_bytes(&bytes);
    let pending = Enrolled {
        credential_id: String::new(),
        secret: b64::encode(&bytes),
        generation: 0,
    };
    remember(store, keychain::KEYPAIR_PENDING, &pending)?;

    let enrolment = match api.keypair_enroll(secret, &b64::encode(&public_key_der(&key))) {
        Ok(enrolment) => enrolment,
        Err(error) => {
            let _ = store.delete(keychain::KEYPAIR_PENDING);
            return Err(error);
        }
    };
    let enrolled = Enrolled {
        credential_id: enrolment.credential_id,
        generation: enrolment.generation,
        ..pending
    };
    remember(store, keychain::KEYPAIR, &enrolled)?;
    let _ = store.delete(keychain::KEYPAIR_PENDING);
    Ok(enrolled)
}

fn remember(store: &dyn Keychain, item: &str, enrolled: &Enrolled) -> Result<()> {
    let json = serde_json::to_string(enrolled)
        .map_err(|e| CloudError::Credential(format!("the key could not be stored: {e}")))?;
    store.set(item, &json)
}

/// What the key signs, exactly.
pub fn message(credential_id: &str, nonce: &str, generation: u64) -> String {
    format!("{MESSAGE_PREFIX}\n{credential_id}\n{nonce}\n{generation}")
}

pub fn sign_message(key: &SigningKey, message: &str) -> String {
    b64::encode(&key.sign(message.as_bytes()).to_bytes())
}

pub fn public_key_der(key: &SigningKey) -> Vec<u8> {
    let mut der = SPKI_PREFIX.to_vec();
    der.extend_from_slice(&key.verifying_key().to_bytes());
    der
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Verifier, VerifyingKey};

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[3u8; 32])
    }

    #[test]
    fn the_signed_message_is_the_four_lines_the_server_rebuilds() {
        assert_eq!(
            message("pcm_FAKE", "nonce-1", 7),
            "penv-cloud:keypair:v1\npcm_FAKE\nnonce-1\n7"
        );
    }

    #[test]
    fn the_signature_verifies_against_the_public_key() {
        let key = key();
        let text = message("pcm_FAKE", "nonce-1", 7);
        let signature = b64::decode(&sign_message(&key, &text)).expect("base64");
        let signature = ed25519_dalek::Signature::from_bytes(
            &<[u8; 64]>::try_from(signature.as_slice()).unwrap(),
        );
        assert!(
            key.verifying_key()
                .verify(text.as_bytes(), &signature)
                .is_ok()
        );
    }

    #[test]
    fn the_public_key_is_spki_der() {
        let der = public_key_der(&key());
        assert_eq!(der.len(), 44);
        assert_eq!(&der[..12], &SPKI_PREFIX);
        let parsed = VerifyingKey::from_bytes(&<[u8; 32]>::try_from(&der[12..]).unwrap()).unwrap();
        assert_eq!(parsed, key().verifying_key());
    }

    #[test]
    fn an_enrolled_key_never_prints_its_secret() {
        let enrolled = Enrolled {
            credential_id: "pcm_FAKE".into(),
            secret: b64::encode(&[3u8; 32]),
            generation: 1,
        };
        let shown = format!("{enrolled:?}");
        assert!(!shown.contains(&enrolled.secret), "{shown}");
        assert!(shown.contains("pcm_FAKE"), "{shown}");
    }
}
