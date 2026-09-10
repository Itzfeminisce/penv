//! Ed25519 over the release checksum file: the pair the owner mints, the
//! signature CI writes beside a `.sha256`, and the check `upgrade` makes before
//! it trusts a digest. Keys and signatures are the raw bytes, base64.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

use crate::b64;
use crate::error::{CloudError, Result};

/// A fresh pair, both halves base64. The private half is the CI secret; the
/// public half ships in the binary and in the installers.
pub struct Keypair {
    pub private: String,
    pub public: String,
}

pub fn generate() -> Result<Keypair> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|e| CloudError::Credential(format!("no key could be generated: {e}")))?;
    let key = SigningKey::from_bytes(&bytes);
    Ok(Keypair {
        private: b64::encode(&bytes),
        public: b64::encode(&key.verifying_key().to_bytes()),
    })
}

/// The signature over `bytes`, base64. None when the private key is not 32 bytes
/// of base64.
pub fn sign(private: &str, bytes: &[u8]) -> Option<String> {
    let key = SigningKey::from_bytes(&raw(private)?);
    Some(b64::encode(&key.sign(bytes).to_bytes()))
}

/// True when this public key signed exactly these bytes.
pub fn verify(public: &str, bytes: &[u8], signature: &str) -> bool {
    let Some(key) = verifying_key(public) else {
        return false;
    };
    let Some(signature) = b64::decode(signature)
        .and_then(|bytes| <[u8; 64]>::try_from(bytes.as_slice()).ok())
        .map(|bytes| Signature::from_bytes(&bytes))
    else {
        return false;
    };
    key.verify(bytes, &signature).is_ok()
}

/// True when any listed key signed the bytes, which is what a rotation needs.
/// An empty list verifies nothing.
pub fn verify_any(keys: &[&str], bytes: &[u8], signature: &str) -> bool {
    keys.iter().any(|key| verify(key, bytes, signature))
}

/// True when the text is a public key at all, whatever it has signed.
pub fn is_public_key(text: &str) -> bool {
    verifying_key(text).is_some()
}

fn verifying_key(public: &str) -> Option<VerifyingKey> {
    VerifyingKey::from_bytes(&raw(public)?).ok()
}

fn raw(text: &str) -> Option<[u8; 32]> {
    b64::decode(text.trim()).and_then(|bytes| <[u8; 32]>::try_from(bytes.as_slice()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUMS: &[u8] = b"0000000000000000000000000000000000000000000000000000000000000000  penv-v1.2.3-x86_64-apple-darwin\n";

    #[test]
    fn a_signature_verifies_against_the_key_that_made_it_and_no_other() {
        let mine = generate().unwrap();
        let theirs = generate().unwrap();
        let signature = sign(&mine.private, SUMS).unwrap();

        assert!(verify(&mine.public, SUMS, &signature));
        assert!(!verify(&theirs.public, SUMS, &signature));
        assert!(!verify(&mine.public, b"another file", &signature));
        assert!(verify_any(
            &[&theirs.public, &mine.public],
            SUMS,
            &signature
        ));
        assert!(!verify_any(&[], SUMS, &signature));
    }

    #[test]
    fn nothing_that_is_not_a_key_or_a_signature_verifies() {
        let mine = generate().unwrap();
        let signature = sign(&mine.private, SUMS).unwrap();

        assert!(!verify("not base64 at all", SUMS, &signature));
        assert!(!verify(&b64::encode(&[7u8; 16]), SUMS, &signature));
        assert!(!verify(&mine.public, SUMS, "nonsense"));
        assert!(!verify(&mine.public, SUMS, &b64::encode(&[0u8; 64])));
        assert_eq!(sign("short", SUMS), None);
    }

    #[test]
    fn a_public_key_is_recognised_before_anything_is_verified() {
        assert!(is_public_key(&generate().unwrap().public));
        assert!(!is_public_key(""));
        assert!(!is_public_key(&b64::encode(&[1u8; 31])));
    }
}
