//! Secret settings (a token, a webhook's address): kept in the system's
//! keychain, never in library.db, a log, a run's environment or its run
//! line. The Windows Credential Manager, the macOS Keychain, or the Secret
//! Service on Linux (GNOME Keyring, KWallet) holds each under the service
//! `AutoScene plugin <id> (<library>)` and the setting's key. `<library>` is
//! the id of the library whose plugin set it (see [`crate::store`]): two
//! AutoScenes with data folders of their own on one account, a test build
//! beside the person's, never read, replace or remove each other's. The
//! store only lists which keys have one; a plugin reads a value through
//! `secrets.get`, for a key of its own secret settings.

use std::collections::HashMap;

use parking_lot::Mutex;

use as_core::msg;

use crate::error::{PluginError, PluginResult};
use crate::manifest::PluginId;

/// The longest secret, in UTF-16 units: a character outside the Basic
/// Multilingual Plane (an emoji) counts twice. So it fits the 2560 bytes a
/// Windows credential holds, where it is kept as UTF-16.
pub const MAX_SECRET_CHARS: usize = 1024;

/// Where secrets are kept, each of a plugin of one library. Each call may
/// wait for the system (a locked keychain asks the person), so none is
/// made on the UI's thread.
pub trait SecretStore: Send + Sync {
    /// The secret of `plugin` of `library` under `key`, or none.
    fn get(&self, library: &str, plugin: &PluginId, key: &str) -> PluginResult<Option<String>>;
    fn set(&self, library: &str, plugin: &PluginId, key: &str, value: &str) -> PluginResult<()>;
    /// Removes it; one that is gone already is no error.
    fn delete(&self, library: &str, plugin: &PluginId, key: &str) -> PluginResult<()>;
}

/// The system's keychain.
pub struct OsKeyring;

impl OsKeyring {
    fn entry(library: &str, plugin: &PluginId, key: &str) -> PluginResult<keyring::Entry> {
        keyring::Entry::new(&service(library, plugin), key).map_err(keychain)
    }
}

impl SecretStore for OsKeyring {
    fn get(&self, library: &str, plugin: &PluginId, key: &str) -> PluginResult<Option<String>> {
        match Self::entry(library, plugin, key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keychain(e)),
        }
    }

    fn set(&self, library: &str, plugin: &PluginId, key: &str, value: &str) -> PluginResult<()> {
        Self::entry(library, plugin, key)?.set_password(value).map_err(keychain)
    }

    fn delete(&self, library: &str, plugin: &PluginId, key: &str) -> PluginResult<()> {
        match Self::entry(library, plugin, key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keychain(e)),
        }
    }
}

/// The keychain's service the secrets of `plugin` of `library` are kept
/// under. On Windows an entry is named `<key>.<service>`: a key has no dot,
/// so no two entries share a name.
fn service(library: &str, plugin: &PluginId) -> String {
    format!("AutoScene plugin {plugin} ({library})")
}

/// What the keychain said: a plain reason for the person, and its own words
/// for the log. Neither carries a value: one that doesn't read is dropped,
/// not shown.
fn keychain(e: keyring::Error) -> PluginError {
    let (reason, said) = match &e {
        // Looked for once per run of AutoScene (keyring's v1 store).
        keyring::Error::NoDefaultStore => {
            (msg!("keychain.noStore", "this computer has none running. Once one runs, restart AutoScene."), true)
        }
        keyring::Error::NoStorageAccess(_) => (msg!("keychain.locked", "it is locked, or refused AutoScene."), true),
        // Their words would hold what was read.
        keyring::Error::BadEncoding(_) => (msg!("keychain.notText", "a secret it holds isn't text."), false),
        keyring::Error::BadDataFormat(..) => (msg!("keychain.damaged", "a secret it holds is damaged."), false),
        keyring::Error::Ambiguous(_) => (msg!("keychain.twice", "it holds this secret more than once."), true),
        keyring::Error::TooLong(..) | keyring::Error::Invalid(..) => {
            (msg!("keychain.refusedName", "it refused the secret's name."), true)
        }
        _ => (msg!("keychain.error", "it reported an error."), true),
    };
    PluginError::Keychain { reason, detail: said.then(|| e.to_string()) }
}

/// Secrets kept in memory, for tests and mocks.
#[derive(Default)]
pub struct MemorySecrets(Mutex<HashMap<(String, PluginId, String), String>>);

impl MemorySecrets {
    /// How many secrets it holds.
    pub fn len(&self) -> usize {
        self.0.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.lock().is_empty()
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, library: &str, plugin: &PluginId, key: &str) -> PluginResult<Option<String>> {
        Ok(self.0.lock().get(&(library.to_owned(), plugin.clone(), key.to_owned())).cloned())
    }

    fn set(&self, library: &str, plugin: &PluginId, key: &str, value: &str) -> PluginResult<()> {
        self.0.lock().insert((library.to_owned(), plugin.clone(), key.to_owned()), value.to_owned());
        Ok(())
    }

    fn delete(&self, library: &str, plugin: &PluginId, key: &str) -> PluginResult<()> {
        self.0.lock().remove(&(library.to_owned(), plugin.clone(), key.to_owned()));
        Ok(())
    }
}

/// Whether `value` may be kept as a secret: 1 to [`MAX_SECRET_CHARS`]
/// characters, without NUL.
pub fn check(value: &str) -> PluginResult<()> {
    if value.is_empty() {
        return Err(PluginError::Invalid(msg!("plugin.secretEmpty", "A secret can't be empty. Remove it instead.")));
    }
    if value.encode_utf16().count() > MAX_SECRET_CHARS {
        return Err(PluginError::Invalid(msg!(
            "plugin.secretTooLong",
            "A secret can be at most {{max}} characters.",
            max = MAX_SECRET_CHARS
        )));
    }
    if value.contains('\0') {
        return Err(PluginError::Invalid(msg!("plugin.secretNul", "A secret can't hold a NUL character.")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use as_core::ToMsg;

    use super::*;

    #[test]
    fn a_keychain_that_refuses_says_why_in_a_sentence_of_its_own() {
        let locked = keychain(keyring::Error::NoDefaultStore);
        assert_eq!(locked.msg().key, "keychain.unusable");
        assert_eq!(
            locked.to_string(),
            "Your system's keychain can't be used: this computer has none running. Once one runs, restart AutoScene."
        );
        let PluginError::Keychain { reason, .. } = locked else { panic!() };
        assert_eq!(reason.key, "keychain.noStore");
    }

    #[test]
    fn a_secret_that_is_refused_has_its_key() {
        let key = |value: &str| check(value).unwrap_err().msg().key;
        assert_eq!(key(""), "plugin.secretEmpty");
        assert_eq!(key("a\0b"), "plugin.secretNul");
        assert_eq!(key(&"a".repeat(MAX_SECRET_CHARS + 1)), "plugin.secretTooLong");
    }

    #[test]
    fn a_secret_fits_a_windows_credential() {
        assert!(check("hunter2").is_ok());
        assert!(check(&"a".repeat(MAX_SECRET_CHARS)).is_ok());
        assert!(check(&"\u{1F511}".repeat(MAX_SECRET_CHARS / 2)).is_ok());
        let too_long = check(&"a".repeat(MAX_SECRET_CHARS + 1)).unwrap_err();
        assert_eq!(too_long.to_string(), "A secret can be at most 1024 characters.");
        // An emoji takes two of them.
        assert!(check(&"\u{1F511}".repeat(MAX_SECRET_CHARS / 2 + 1)).is_err());
        assert_eq!(check("").unwrap_err().to_string(), "A secret can't be empty. Remove it instead.");
        assert!(check("a\0b").is_err());
        // In UTF-16, as Windows keeps it: within its 2560 bytes.
        const { assert!(MAX_SECRET_CHARS * 2 <= 2560) };
    }

    #[test]
    fn memory_secrets_are_kept_per_library_plugin_and_key() {
        let (a, b) = (PluginId::parse("dev.a.one").unwrap(), PluginId::parse("dev.a.two").unwrap());
        let secrets = MemorySecrets::default();
        secrets.set("lib", &a, "token", "one").unwrap();
        secrets.set("lib", &b, "token", "two").unwrap();
        secrets.set("other", &a, "token", "three").unwrap();
        assert_eq!(secrets.get("lib", &a, "token").unwrap().as_deref(), Some("one"));
        assert_eq!(secrets.get("lib", &a, "other").unwrap(), None);
        secrets.delete("lib", &a, "token").unwrap();
        secrets.delete("lib", &a, "token").unwrap();
        assert_eq!((secrets.get("lib", &a, "token").unwrap(), secrets.len()), (None, 2));
        assert_eq!(secrets.get("other", &a, "token").unwrap().as_deref(), Some("three"));
    }

    #[test]
    fn each_library_keeps_its_secrets_under_a_service_of_its_own() {
        let (a, b) = (PluginId::parse("dev.a.one").unwrap(), PluginId::parse("dev.a.two").unwrap());
        let lib = "4f1c2b9a-0000-4000-8000-000000000001";
        let other = "4f1c2b9a-0000-4000-8000-000000000002";
        assert_ne!(service(lib, &a), service(other, &a));
        assert_ne!(service(lib, &a), service(lib, &b));
        assert!(service(lib, &a).contains(lib) && service(lib, &a).contains("dev.a.one"));
    }

    #[test]
    fn a_keychain_error_says_why_in_plain_words() {
        let said = |e: keyring::Error| {
            let e = keychain(e);
            (e.to_string(), e.detail())
        };
        let failed = keyring::Error::PlatformFailure("Element not found. (0x80070490)".into());
        assert_eq!(
            said(failed),
            (
                "Your system's keychain can't be used: it reported an error.".to_owned(),
                "keychain: Platform failure: Element not found. (0x80070490)".to_owned()
            )
        );
        let locked = keyring::Error::NoStorageAccess("Locked".into());
        assert_eq!(said(locked).0, "Your system's keychain can't be used: it is locked, or refused AutoScene.");
        // What was read stays out of both.
        let (shown, logged) = said(keyring::Error::BadEncoding(b"hunter2\xff".to_vec()));
        assert_eq!(shown, "Your system's keychain can't be used: a secret it holds isn't text.");
        assert!(!logged.contains("hunter2"), "{logged}");
    }

    /// Removes its entry when it goes, also when the test fails.
    struct Cleanup<'a>(&'a str, &'a PluginId, &'a str);

    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            let _ = OsKeyring.delete(self.0, self.1, self.2);
        }
    }

    /// The system's own keychain, which CI has none of: run it by hand
    /// (`cargo test -p as-plugin os_keyring_round_trip -- --ignored`). It
    /// leaves nothing behind.
    #[test]
    #[ignore = "uses the system keychain"]
    fn os_keyring_round_trip() {
        let id = PluginId::parse("dev.autoscene.keychain-test").unwrap();
        let (library, other) = (uuid::Uuid::now_v7().simple().to_string(), uuid::Uuid::now_v7().simple().to_string());
        let key = "roundTrip";
        let _cleanup = (Cleanup(&library, &id, key), Cleanup(&other, &id, key));
        assert_eq!(OsKeyring.get(&library, &id, key).unwrap(), None);
        OsKeyring.set(&library, &id, key, "first").unwrap();
        assert_eq!(OsKeyring.get(&library, &id, key).unwrap().as_deref(), Some("first"));
        // Another library's is an entry of its own.
        assert_eq!(OsKeyring.get(&other, &id, key).unwrap(), None);
        OsKeyring.set(&other, &id, key, "second").unwrap();
        // Set again, it is replaced; the longest one fits.
        let longest = format!("{}é\u{1F511}", "x".repeat(MAX_SECRET_CHARS - 3));
        check(&longest).unwrap();
        OsKeyring.set(&library, &id, key, &longest).unwrap();
        assert_eq!(OsKeyring.get(&library, &id, key).unwrap(), Some(longest));
        OsKeyring.delete(&library, &id, key).unwrap();
        assert_eq!(OsKeyring.get(&library, &id, key).unwrap(), None);
        assert_eq!(OsKeyring.get(&other, &id, key).unwrap().as_deref(), Some("second"));
        // Gone already is no error.
        OsKeyring.delete(&library, &id, key).unwrap();
        OsKeyring.delete(&other, &id, key).unwrap();
    }
}
