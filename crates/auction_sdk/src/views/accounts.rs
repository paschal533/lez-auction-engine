//! The account picker: one row per role, from the wallet's label convention. A label `<name>-stable`,
//! `<name>-coll` or `<name>-protocol` is a token holding of `<name>`: it is attached to that row and
//! never listed alone. Roles come first, in [`ROLES`] order; other names follow in label order.

use std::collections::BTreeMap;

use lee_core::account::AccountId;
use serde::Serialize;

use crate::{chain::Chain, decode::Reader, error::SdkResult, verify::VerifiedConfig};

/// The roles of the demo story, in the picker's order (`scripts/lib/demo-scenario.sh` labels).
pub const ROLES: [&str; 7] = [
    "owner",
    "keeper",
    "caller",
    "taker",
    "admin",
    "engine-admin",
    "oracle-src",
];

/// Holding suffixes, in the order a row picks its main holding.
const HOLDINGS: [&str; 3] = ["-stable", "-coll", "-protocol"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccountView {
    /// The row's name: the base label (`owner`), also when only its holdings are labelled (`taker`).
    pub label: String,
    /// The base label's account, else the main holding's.
    pub id: AccountId,
    /// The role, when the name is one of [`ROLES`].
    pub role: Option<&'static str>,
    /// Balance of the `<name>-stable` holding (`None`: no such label, or not initialised yet).
    #[serde(with = "crate::dec::opt")]
    pub stable_balance: Option<u128>,
    #[serde(with = "crate::dec::opt")]
    pub coll_balance: Option<u128>,
    /// Label of the main holding (`-stable`, else `-coll`, else `-protocol`).
    pub holding: Option<String>,
}

#[derive(Default)]
struct Group {
    base: Option<AccountId>,
    /// suffix -> (label, id)
    holdings: BTreeMap<&'static str, (String, AccountId)>,
}

fn split_holding(label: &str) -> Option<(&str, &'static str)> {
    HOLDINGS.iter().find_map(|suffix| {
        label
            .strip_suffix(suffix)
            .filter(|base| !base.is_empty())
            .map(|base| (base, *suffix))
    })
}

fn group(labels: &[(String, AccountId)]) -> BTreeMap<String, Group> {
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for (label, id) in labels {
        match split_holding(label) {
            Some((base, suffix)) => {
                groups
                    .entry(base.to_owned())
                    .or_default()
                    .holdings
                    .insert(suffix, (label.clone(), *id));
            }
            None => groups.entry(label.clone()).or_default().base = Some(*id),
        }
    }
    groups
}

fn row<C: Chain + ?Sized>(
    r: &Reader<'_, C>,
    v: &VerifiedConfig,
    name: &str,
    g: &Group,
) -> SdkResult<Option<AccountView>> {
    let main = HOLDINGS.iter().find_map(|s| g.holdings.get(s));
    let Some(id) = g.base.or_else(|| main.map(|(_, id)| *id)) else {
        return Ok(None);
    };
    let balance = |suffix: &str, definition| match g.holdings.get(suffix) {
        Some((_, holding)) => r.balance(holding, &v.state.token_program_id, definition),
        None => Ok(None),
    };
    Ok(Some(AccountView {
        label: name.to_owned(),
        id,
        role: ROLES.iter().find(|role| **role == name).copied(),
        stable_balance: balance("-stable", &v.state.stablecoin_definition_id)?,
        coll_balance: balance("-coll", &v.state.collateral_definition_id)?,
        holding: main.map(|(label, _)| label.clone()),
    }))
}

/// The picker rows for the wallet's labels (e.g. `Keystore::labels`), with current balances.
pub fn accounts<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    labels: &[(String, AccountId)],
) -> SdkResult<Vec<AccountView>> {
    let r = Reader::new(chain, v.engine.program);
    let groups = group(labels);
    let roles = ROLES.iter().filter_map(|role| groups.get_key_value(*role));
    let others = groups
        .iter()
        .filter(|(name, _)| !ROLES.contains(&name.as_str()));
    let mut rows = Vec::new();
    for (name, g) in roles.chain(others) {
        if let Some(view) = row(&r, v, name, g)? {
            rows.push(view);
        }
    }
    Ok(rows)
}
