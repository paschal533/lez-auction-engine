//! The LEZ wallet keystore (`$LEE_WALLET_HOME_DIR/storage.json`, LEZ v0.2.4 `wallet::storage`), read
//! directly (ADR-041). Public accounts sign with the stored `ssk` (the key the wallet itself signs
//! with: `impl From<&ChildKeysPublic> for &PrivateKey` returns `&value.ssk`); imported public accounts
//! with `pub_sign_key`. Private accounts are recorded only so they can be refused: the PoC submits
//! public transactions (spec §8). The v0.2.4 wallet stores keys unencrypted (its password argument is
//! unused upstream); this module never prints, logs or returns key material in errors, `Display` or
//! `Debug`. Paths and labels may appear in messages; private keys, seeds and mnemonics never do.
//!
//! Imported private wallet entries (`ImportedPrivate`) carry no `account_id` upstream, so they are
//! skipped: an unlabelled imported private account fails as `NoKey`, not `PrivateAccount`. A labelled
//! one (`labels: {l: {"Private": id}}`) is refused as `PrivateAccount`. Shared private accounts
//! (`key_chain.shared_private_accounts`, keyed by account id) are refused as `PrivateAccount` too.
//!
//! Key hygiene: `lee::PrivateKey` does not implement `Zeroize` upstream, so the in-memory keys cannot
//! be wiped by this crate; the file text is dropped as soon as it is parsed and nothing is written to
//! disk.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use lee::{PrivateKey, PublicKey};
use lee_core::account::AccountId;
use serde_json::Value;

pub const HOME_ENV: &str = "LEE_WALLET_HOME_DIR";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeystoreError {
    #[error("LEE_WALLET_HOME_DIR is not set and no --wallet-home was given")]
    HomeNotSet,
    #[error("wallet storage not found at {0} (create accounts with `wallet account new public`)")]
    NotFound(String),
    #[error("cannot read wallet storage {path}: {reason}")]
    Unreadable { path: String, reason: String },
    #[error("wallet storage {path} is malformed: {reason}")]
    Malformed { path: String, reason: String },
    #[error("no wallet account is labelled `{0}`")]
    UnknownLabel(String),
    #[error("account {0} is not in the wallet (no signing key)")]
    NoKey(String),
    #[error("account {0} is a private account; the PoC signs public transactions only (spec §8)")]
    PrivateAccount(String),
    #[error("the wallet entry for {0} holds a key that does not derive that account id")]
    KeyMismatch(String),
}

pub struct Keystore {
    path: PathBuf,
    keys: BTreeMap<AccountId, PrivateKey>,
    private: BTreeSet<AccountId>,
    labels: BTreeMap<String, AccountId>,
}

/// Lists the path, account ids and labels only; never a key.
impl std::fmt::Debug for Keystore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keystore")
            .field("path", &self.path)
            .field("public_accounts", &self.keys.keys().collect::<Vec<_>>())
            .field("private_accounts", &self.private.len())
            .field("labels", &self.labels.keys().collect::<Vec<_>>())
            .finish()
    }
}

fn insert(
    keys: &mut BTreeMap<AccountId, PrivateKey>,
    id: AccountId,
    key: PrivateKey,
) -> Result<(), KeystoreError> {
    if AccountId::from(&PublicKey::new_from_private_key(&key)) != id {
        return Err(KeystoreError::KeyMismatch(id.to_string()));
    }
    keys.insert(id, key);
    Ok(())
}

impl Keystore {
    /// `--wallet-home` wins over `$LEE_WALLET_HOME_DIR`.
    pub fn default_path(wallet_home: Option<&Path>) -> Result<PathBuf, KeystoreError> {
        Self::path_from(wallet_home, std::env::var_os(HOME_ENV))
    }

    pub fn path_from(
        wallet_home: Option<&Path>,
        env_home: Option<OsString>,
    ) -> Result<PathBuf, KeystoreError> {
        let home = match wallet_home {
            Some(h) => h.to_path_buf(),
            None => env_home
                .map(PathBuf::from)
                .ok_or(KeystoreError::HomeNotSet)?,
        };
        Ok(home.join("storage.json"))
    }

    pub fn open(path: &Path) -> Result<Self, KeystoreError> {
        let shown = path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                KeystoreError::NotFound(shown.clone())
            } else {
                KeystoreError::Unreadable {
                    path: shown.clone(),
                    reason: e.kind().to_string(),
                }
            }
        })?;
        Self::parse(&text, path)
    }

    pub fn parse(text: &str, path: &Path) -> Result<Self, KeystoreError> {
        let shown = path.display().to_string();
        let malformed = |reason: String| KeystoreError::Malformed {
            path: shown.clone(),
            reason,
        };
        // serde_json syntax errors name a line and a column, never the content.
        let v: Value = serde_json::from_str(text).map_err(|e| malformed(e.to_string()))?;
        let entries = v
            .pointer("/key_chain/accounts")
            .and_then(Value::as_array)
            .ok_or_else(|| malformed("key_chain.accounts is missing".into()))?;
        let mut keys = BTreeMap::new();
        let mut private = BTreeSet::new();
        for (i, entry) in entries.iter().enumerate() {
            let (kind, body) = entry
                .as_object()
                .and_then(|o| o.iter().next())
                .ok_or_else(|| malformed(format!("account entry {i} is not an object")))?;
            let account = |field: &str| {
                body.get(field)
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse::<AccountId>().ok())
                    .ok_or_else(|| {
                        malformed(format!("account entry {i}: missing or invalid {field}"))
                    })
            };
            let signing_key = |pointer: &str| {
                body.pointer(pointer)
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse::<PrivateKey>().ok())
                    .ok_or_else(|| {
                        malformed(format!("account entry {i}: missing or invalid signing key"))
                    })
            };
            match kind.as_str() {
                "Public" => insert(&mut keys, account("account_id")?, signing_key("/data/ssk")?)?,
                "ImportedPublic" => {
                    insert(
                        &mut keys,
                        account("account_id")?,
                        signing_key("/pub_sign_key")?,
                    )?;
                }
                // `ImportedPrivate` has no `account_id` upstream: it parses to nothing here.
                "Private" => {
                    private.insert(account("account_id")?);
                }
                "ImportedPrivate" => {}
                other => {
                    return Err(malformed(format!(
                        "account entry {i}: unknown kind {other}"
                    )))
                }
            }
        }
        // `shared_private_accounts` is a map keyed by account id (upstream `SharedAccountEntry`).
        if let Some(shared) = v
            .pointer("/key_chain/shared_private_accounts")
            .and_then(Value::as_object)
        {
            for raw in shared.keys() {
                let id: AccountId = raw
                    .parse()
                    .map_err(|_| malformed("shared_private_accounts: invalid account id".into()))?;
                private.insert(id);
            }
        }
        let mut labels = BTreeMap::new();
        if let Some(map) = v.get("labels").and_then(Value::as_object) {
            for (label, target) in map {
                let (kind, raw) = target
                    .as_object()
                    .and_then(|o| o.iter().next())
                    .and_then(|(k, v)| v.as_str().map(|s| (k.as_str(), s)))
                    .ok_or_else(|| malformed(format!("label {label} is malformed")))?;
                let id: AccountId = raw
                    .parse()
                    .map_err(|_| malformed(format!("label {label}: invalid account id")))?;
                match kind {
                    "Public" => {}
                    "Private" => {
                        private.insert(id);
                    }
                    other => return Err(malformed(format!("label {label}: unknown kind {other}"))),
                }
                labels.insert(label.clone(), id);
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            keys,
            private,
            labels,
        })
    }

    /// A wallet label, or a base58 account id (recipients need no key). Private accounts are refused.
    pub fn resolve(&self, label_or_id: &str) -> Result<AccountId, KeystoreError> {
        let id = match self.labels.get(label_or_id) {
            Some(id) => *id,
            None => label_or_id
                .parse::<AccountId>()
                .map_err(|_| KeystoreError::UnknownLabel(label_or_id.to_owned()))?,
        };
        if self.private.contains(&id) {
            return Err(KeystoreError::PrivateAccount(id.to_string()));
        }
        Ok(id)
    }

    pub fn key(&self, id: &AccountId) -> Result<&PrivateKey, KeystoreError> {
        if self.private.contains(id) {
            return Err(KeystoreError::PrivateAccount(id.to_string()));
        }
        self.keys
            .get(id)
            .ok_or_else(|| KeystoreError::NoKey(id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{id, key, keystore_json};
    use std::ffi::OsString;

    fn store(json: &str) -> Result<Keystore, KeystoreError> {
        Keystore::parse(json, Path::new("/w/storage.json"))
    }

    #[test]
    fn keystore_resolves_labels_and_ids_and_signs_with_ssk() {
        let ks = store(&keystore_json(&[("keeper", 51), ("caller", 62)])).expect("parses");
        assert_eq!(ks.resolve("keeper").expect("label"), id(51));
        assert_eq!(ks.resolve(&id(62).to_string()).expect("base58 id"), id(62));
        assert_eq!(ks.key(&id(51)).expect("key"), &key(51));
        // A non-signing account (e.g. a recipient) resolves by id even without a key.
        assert_eq!(ks.resolve(&id(9).to_string()).expect("any id"), id(9));
    }

    /// The raw key bytes in the two encodings an error could leak them in: the wallet's hex and base58.
    fn key_encodings(n: u8) -> [String; 2] {
        [
            key(n).to_string(),
            AccountId::new(*key(n).value()).to_string(),
        ]
    }

    #[test]
    fn keystore_errors_are_typed_and_never_print_key_material() {
        let secret = key(50).to_string();
        let home = Keystore::path_from(None, None).expect_err("no home");
        assert_eq!(home, KeystoreError::HomeNotSet);
        assert_eq!(
            Keystore::path_from(None, Some(OsString::from("/w"))).expect("env"),
            PathBuf::from("/w/storage.json")
        );
        let dir = tempfile::tempdir().expect("tmp");
        let missing = Keystore::open(&dir.path().join("none/storage.json")).expect_err("missing");
        assert!(matches!(missing, KeystoreError::NotFound(_)));
        let bad = store("{ not json").expect_err("malformed");
        assert!(matches!(bad, KeystoreError::Malformed { .. }));
        let ks = store(&keystore_json(&[("engine-admin", 50)])).expect("parses");
        assert!(matches!(
            ks.resolve("nobody"),
            Err(KeystoreError::UnknownLabel(_))
        ));
        assert!(matches!(ks.key(&id(51)), Err(KeystoreError::NoKey(_))));
        // A key stored under another account's id.
        let swapped = keystore_json(&[("engine-admin", 50)])
            .replace(&id(50).to_string(), &id(51).to_string());
        let mismatch = store(&swapped).expect_err("key does not derive the id");
        assert!(matches!(mismatch, KeystoreError::KeyMismatch(_)));
        // A private account is refused as a signer and as a resolved account.
        let with_private = format!(
            "{{\"key_chain\":{{\"accounts\":[{{\"Private\":{{\"account_id\":\"{}\"}}}}]}},\"labels\":{{\"secret\":{{\"Private\":\"{}\"}}}}}}",
            id(70),
            id(70)
        );
        let ks2 = store(&with_private).expect("parses");
        assert!(matches!(
            ks2.resolve("secret"),
            Err(KeystoreError::PrivateAccount(_))
        ));
        assert!(matches!(
            ks2.key(&id(70)),
            Err(KeystoreError::PrivateAccount(_))
        ));
        for e in [home, missing, bad, mismatch] {
            assert!(!e.to_string().contains(&secret), "{e}");
        }
        assert!(!format!("{ks:?}").contains(&secret), "Debug lists ids only");
    }

    #[test]
    fn keystore_formats_never_contain_key_bytes_in_any_error_or_debug() {
        // The keystore holds key(50); a malformed copy carries a bad value next to a real key.
        let json = keystore_json(&[("engine-admin", 50), ("keeper", 51)]);
        let ks = store(&json).expect("parses");
        let swapped = json.replace(&id(50).to_string(), &id(51).to_string());
        let unreadable = KeystoreError::Unreadable {
            path: "/w/storage.json".into(),
            reason: "permission denied".into(),
        };
        let truncated = json.get(..json.len() / 2).expect("ascii json").to_owned();
        let errors = [
            KeystoreError::HomeNotSet,
            KeystoreError::NotFound("/w/storage.json".into()),
            unreadable,
            store(&truncated).expect_err("truncated"),
            store(&json.replace("\"Public\":{", "\"Mystery\":{")).expect_err("unknown kind"),
            store(&json.replace(&key(50).to_string(), "zz")).expect_err("bad key"),
            ks.resolve("nobody").expect_err("unknown label"),
            ks.key(&id(9)).expect_err("no key"),
            store(&swapped).expect_err("mismatch"),
            KeystoreError::PrivateAccount(id(70).to_string()),
        ];
        let mut rendered: Vec<String> = errors
            .iter()
            .flat_map(|e| [format!("{e}"), format!("{e:?}")])
            .collect();
        rendered.push(format!("{ks:?}"));
        rendered.push(format!("{ks:#?}"));
        for n in [50, 51] {
            for secret in key_encodings(n) {
                for text in &rendered {
                    assert!(!text.contains(&secret), "key {n} leaked in: {text}");
                }
            }
        }
        // The Public entry's raw `sk` (not the signing key) must not leak either.
        for n in [150, 151] {
            for secret in key_encodings(n) {
                for text in &rendered {
                    assert!(!text.contains(&secret), "sk {n} leaked in: {text}");
                }
            }
        }
        assert!(rendered.iter().any(|t| t.contains("storage.json")));
    }

    #[test]
    fn imported_public_accounts_sign_and_unlabelled_imported_private_is_no_key() {
        let imported = format!(
            "{{\"key_chain\":{{\"accounts\":[{{\"ImportedPublic\":{{\"account_id\":\"{}\",\"pub_sign_key\":\"{}\"}}}},{{\"ImportedPrivate\":{{\"identifier\":1}}}}]}},\"labels\":{{}}}}",
            id(33),
            key(33)
        );
        let ks = store(&imported).expect("parses");
        assert_eq!(ks.key(&id(33)).expect("imported key"), &key(33));
        assert!(matches!(ks.key(&id(34)), Err(KeystoreError::NoKey(_))));
        let wrong = imported.replace(&id(33).to_string(), &id(34).to_string());
        assert!(matches!(store(&wrong), Err(KeystoreError::KeyMismatch(_))));
    }

    #[test]
    fn shared_private_accounts_are_refused_and_a_malformed_private_entry_fails() {
        let shared = format!(
            "{{\"key_chain\":{{\"accounts\":[],\"shared_private_accounts\":{{\"{}\":{{}}}}}},\"labels\":{{}}}}",
            id(80)
        );
        let ks = store(&shared).expect("parses");
        assert!(matches!(
            ks.resolve(&id(80).to_string()),
            Err(KeystoreError::PrivateAccount(_))
        ));
        assert!(matches!(
            ks.key(&id(80)),
            Err(KeystoreError::PrivateAccount(_))
        ));
        let bad_shared = shared.replace(&id(80).to_string(), "not-an-id");
        assert!(matches!(
            store(&bad_shared),
            Err(KeystoreError::Malformed { .. })
        ));
        let bad_private =
            "{\"key_chain\":{\"accounts\":[{\"Private\":{\"account_id\":\"nope\"}}]},\"labels\":{}}";
        assert!(matches!(
            store(bad_private),
            Err(KeystoreError::Malformed { .. })
        ));
    }
}
