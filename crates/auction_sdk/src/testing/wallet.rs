//! Wallet-format keystores and in-memory key sources for tests (never real keys: seeds `[n; 32]`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lee::{PrivateKey, PublicKey};
use lee_core::account::AccountId;
use serde_json::{json, Map, Value};

use super::{id, key};
use crate::{
    error::{SdkError, SdkResult},
    keystore::KeystoreError,
    tx::KeySource,
};

/// The labels the CLI golden tests and the standard fixture use.
pub const STANDARD_LABELS: [(&str, u8); 14] = [
    ("engine-admin", 50),
    ("keeper", 51),
    ("keeper-coll", 52),
    ("bidder-stable", 53),
    ("bidder-coll", 54),
    ("owner", 41),
    ("owner-coll", 42),
    ("owner-stable", 43),
    ("host-admin", 44),
    ("freeze", 45),
    ("caller", 62),
    ("taker-stable", 57),
    ("taker-protocol", 58),
    ("owner2", 65),
];

/// A `storage.json` in the LEZ v0.2.4 wallet format (`PersistentStorage`) with one public account per
/// entry. `data` mirrors `ChildKeysPublic` (`sk`, `ssk`, `pk`, `cc`, `cci`); the test key is the `ssk`
/// (the signing key the account id derives from), so `sk` is a distinct dummy.
#[must_use]
pub fn keystore_json(entries: &[(&str, u8)]) -> String {
    let accounts: Vec<Value> = entries
        .iter()
        .map(|(_, n)| {
            json!({ "Public": {
                "account_id": id(*n).to_string(),
                "chain_index": [],
                "data": {
                    "sk": key(n.wrapping_add(100)).to_string(),
                    "ssk": key(*n).to_string(),
                    "pk": PublicKey::new_from_private_key(&key(*n)).to_string(),
                    "cc": vec![0u8; 32],
                    "cci": Value::Null
                }
            }})
        })
        .collect();
    let labels: Map<String, Value> = entries
        .iter()
        .map(|(l, n)| ((*l).to_owned(), json!({ "Public": id(*n).to_string() })))
        .collect();
    json!({ "key_chain": { "accounts": accounts }, "last_synced_block": 0, "labels": labels })
        .to_string()
}

/// Writes `keystore_json(entries)` to `<dir>/storage.json`.
pub fn write_keystore(dir: &Path, entries: &[(&str, u8)]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("storage.json");
    std::fs::write(&path, keystore_json(entries))?;
    Ok(path)
}

/// In-memory signing keys for the test seeds `[n; 32]`.
pub struct TestKeys(pub BTreeMap<AccountId, PrivateKey>);

impl TestKeys {
    #[must_use]
    pub fn of(seeds: &[u8]) -> Self {
        Self(seeds.iter().map(|n| (id(*n), key(*n))).collect())
    }
}

impl KeySource for TestKeys {
    fn signing_key(&self, id: &AccountId) -> SdkResult<&PrivateKey> {
        self.0
            .get(id)
            .ok_or_else(|| SdkError::Keystore(KeystoreError::NoKey(id.to_string())))
    }
}
