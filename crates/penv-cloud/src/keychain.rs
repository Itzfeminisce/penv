use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::error::{CloudError, Result};

/// The one service name every penv item is filed under.
pub const SERVICE: &str = "penv";

/// The person's `pcu_` credential.
pub const USER: &str = "user";
/// The 32 bytes the cache is encrypted with.
pub const CACHE_KEY: &str = "cache-key";
/// The enrolled Ed25519 key, its credential id and its generation counter.
pub const KEYPAIR: &str = "keypair";

/// The account one item is stored under. Two servers never share a credential.
pub fn account(base_url: &str, item: &str) -> String {
    format!("{base_url}/{item}")
}

/// One secret store, so the cache and every credential kind can be driven from
/// a map in a test.
pub trait Keychain {
    fn get(&self, item: &str) -> Result<Option<String>>;
    fn set(&self, item: &str, value: &str) -> Result<()>;
    fn delete(&self, item: &str) -> Result<()>;

    /// False where the host has no store. The cache is off there, and every run
    /// goes to the server.
    fn usable(&self) -> bool {
        true
    }
}

/// The OS keychain.
pub struct Keyring {
    base_url: String,
}

impl Keyring {
    /// `None` where the platform store will not open, which is normal in a
    /// container. The caller falls back to [`NoKeychain`].
    pub fn open(base_url: &str) -> Option<Keyring> {
        keyring::Entry::store_status().as_ref().ok()?;
        Some(Keyring {
            base_url: base_url.to_string(),
        })
    }

    fn entry(&self, item: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, &account(&self.base_url, item))
            .map_err(|e| CloudError::Keychain(e.to_string()))
    }
}

impl Keychain for Keyring {
    fn get(&self, item: &str) -> Result<Option<String>> {
        match self.entry(item)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(CloudError::Keychain(e.to_string())),
        }
    }

    fn set(&self, item: &str, value: &str) -> Result<()> {
        self.entry(item)?
            .set_password(value)
            .map_err(|e| CloudError::Keychain(e.to_string()))
    }

    fn delete(&self, item: &str) -> Result<()> {
        match self.entry(item)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CloudError::Keychain(e.to_string())),
        }
    }
}

/// A store that holds nothing. Reads answer empty so credential resolution walks
/// on; writes say why they cannot happen.
pub struct NoKeychain;

impl Keychain for NoKeychain {
    fn get(&self, _item: &str) -> Result<Option<String>> {
        Ok(None)
    }

    fn set(&self, _item: &str, _value: &str) -> Result<()> {
        Err(CloudError::Keychain(
            "this host has no keychain to write to".into(),
        ))
    }

    fn delete(&self, _item: &str) -> Result<()> {
        Ok(())
    }

    fn usable(&self) -> bool {
        false
    }
}

/// A store in memory, for tests.
#[derive(Default)]
pub struct MemoryKeychain(Mutex<BTreeMap<String, String>>);

impl MemoryKeychain {
    pub fn new() -> MemoryKeychain {
        MemoryKeychain::default()
    }

    pub fn items(&self) -> BTreeMap<String, String> {
        self.0.lock().expect("the test keychain").clone()
    }
}

impl Keychain for MemoryKeychain {
    fn get(&self, item: &str) -> Result<Option<String>> {
        Ok(self.0.lock().expect("the test keychain").get(item).cloned())
    }

    fn set(&self, item: &str, value: &str) -> Result<()> {
        self.0
            .lock()
            .expect("the test keychain")
            .insert(item.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, item: &str) -> Result<()> {
        self.0.lock().expect("the test keychain").remove(item);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_is_filed_under_the_server_it_belongs_to() {
        assert_eq!(
            account("https://penv.cloud", USER),
            "https://penv.cloud/user"
        );
        assert_ne!(
            account("https://penv.cloud", USER),
            account("http://127.0.0.1:8080", USER)
        );
    }

    #[test]
    fn a_host_with_no_keychain_reads_empty_and_refuses_to_write() {
        let store = NoKeychain;
        assert_eq!(store.get(USER).unwrap(), None);
        assert!(!store.usable());
        assert!(store.set(USER, "pcu_FAKE").is_err());
    }
}
