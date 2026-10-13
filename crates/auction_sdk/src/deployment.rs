//! Machine-readable deployment (I4): the ImageIDs a client trusts. `deployments/<network>.json` must
//! agree with `DEPLOYMENTS.md` (test `localnet_json_matches_deployments_md`).

use std::path::Path;

use lee_core::{account::AccountId, program::ProgramId};
use serde::{Deserialize, Serialize};

use crate::error::{SdkError, SdkResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentFile {
    pub network: String,
    pub sequencer_url: String,
    pub engine_program_id: String,
    pub host_program_id: String,
    pub token_program_id: String,
    #[serde(default)]
    pub twap_oracle_program_id: Option<String>,
    #[serde(default)]
    pub engine_admin: Option<String>,
    #[serde(default)]
    pub config: Option<String>,
    #[serde(default)]
    pub chain_genesis_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployment {
    pub network: String,
    pub sequencer_url: String,
    pub engine_program: ProgramId,
    pub host_program: ProgramId,
    pub token_program: ProgramId,
    pub twap_oracle_program: Option<ProgramId>,
    /// When set, `verify_config` also requires this config admin (the expected operator, ADR-027).
    pub engine_admin: Option<AccountId>,
    /// Default config for commands that do not name one.
    pub config: Option<AccountId>,
    /// Hex hash of block 1 of the chain this file was recorded on (ADR-046); state-changing dev commands require it.
    pub chain_genesis: Option<String>,
}

/// ImageID as 64 hex characters (little-endian limbs, the `DEPLOYMENTS.md` / `spel program-id` form).
#[must_use]
pub fn program_id_hex(id: &ProgramId) -> String {
    id.iter()
        .flat_map(|l| l.to_le_bytes())
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn program_id_from_hex(s: &str) -> Result<ProgramId, String> {
    let bytes = hex::decode(s.trim()).map_err(|e| format!("not hex: {e}"))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|b: Vec<u8>| format!("an ImageID is 32 bytes, got {}", b.len()))?;
    let mut id = [0u32; 8];
    for (limb, c) in id.iter_mut().zip(arr.chunks_exact(4)) {
        *limb = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
    }
    Ok(id)
}

impl Deployment {
    pub fn from_json(text: &str, path: &str) -> SdkResult<Self> {
        let bad = |reason: String| SdkError::Deployment {
            path: path.to_owned(),
            reason,
        };
        let f: DeploymentFile = serde_json::from_str(text).map_err(|e| bad(e.to_string()))?;
        let pid =
            |s: &str, what: &str| program_id_from_hex(s).map_err(|e| bad(format!("{what}: {e}")));
        let acct = |s: &str, what: &str| {
            s.parse::<AccountId>()
                .map_err(|_| bad(format!("{what} is not a base58 account id")))
        };
        Ok(Self {
            network: f.network.clone(),
            sequencer_url: f.sequencer_url.clone(),
            engine_program: pid(&f.engine_program_id, "engine_program_id")?,
            host_program: pid(&f.host_program_id, "host_program_id")?,
            token_program: pid(&f.token_program_id, "token_program_id")?,
            twap_oracle_program: f
                .twap_oracle_program_id
                .as_deref()
                .map(|s| pid(s, "twap_oracle_program_id"))
                .transpose()?,
            engine_admin: f
                .engine_admin
                .as_deref()
                .map(|s| acct(s, "engine_admin"))
                .transpose()?,
            config: f.config.as_deref().map(|s| acct(s, "config")).transpose()?,
            chain_genesis: f
                .chain_genesis_hash
                .as_deref()
                .map(|h| match hex::decode(h) {
                    Ok(b) if b.len() == 32 && h == h.to_lowercase() => Ok(h.to_owned()),
                    _ => Err(bad(
                        "chain_genesis_hash must be 64 lowercase hex characters".into(),
                    )),
                })
                .transpose()?,
        })
    }

    pub fn load(path: &Path) -> SdkResult<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| SdkError::Deployment {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        Self::from_json(&text, &path.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_id_hex_round_trips_and_rejects_bad_input() {
        let id: ProgramId = [1, 2, 3, 4, 5, 6, 7, 0xFFFF_FFFF];
        assert_eq!(
            program_id_from_hex(&program_id_hex(&id)).expect("round trip"),
            id
        );
        assert!(program_id_from_hex("zz").is_err());
        assert!(program_id_from_hex("abcd").is_err());
        let e = Deployment::from_json("{\"network\":1}", "x.json").expect_err("bad json");
        assert!(e.to_string().contains("x.json"));
    }
}
