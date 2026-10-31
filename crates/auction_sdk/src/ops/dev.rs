//! Dev-network commands: the token transfer and the oracle publish (ADR-050). Both send on the named
//! dev networks only: the caller runs `require_dev_network` first (ADR-046).

use stablecoin_core::ProtocolParameters;

use crate::{
    chain::Chain,
    decode::Reader,
    deployment::Deployment,
    error::{SdkError, SdkResult},
    oracle, plan, plan_host,
    verify::{verify_chain_identity, VerifiedConfig},
};

use super::{
    admin::{require_engine_admin, require_host_admin},
    fact, Accounts, Prepared, Step, CHECK_BALANCE, CHECK_HOST,
};

/// The named dev networks the sending dev commands (mock tokens, mock oracles) may act on: the
/// throwaway localnet and the public LEZ v0.2.4 legacy testnet (`scripts/testnet-legacy.sh`). Matched
/// exactly. A name on this list is never enough on its own: the node's block 1 must also be the one the
/// deployment file recorded (`verify_chain_identity`).
pub const DEV_NETWORKS: &[&str] = &["localnet", "testnet-legacy"];

/// The guard of every dev command that sends: a named dev network in the deployment file AND the chain
/// it recorded (ADR-046; Plan 3a M6: a label alone let `--rpc` aim them at any node).
pub fn require_dev_network<C: Chain + ?Sized>(chain: &C, d: &Deployment) -> SdkResult<()> {
    if !DEV_NETWORKS.contains(&d.network.as_str()) {
        return Err(SdkError::Refused(format!(
            "dev commands that send are for the named dev networks only ({}); the deployment file names network {:?}",
            DEV_NETWORKS.join(", "),
            d.network
        )));
    }
    verify_chain_identity(chain, d)
}

/// The flags of `auction-cli dev token-transfer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenTransferRequest {
    pub from: String,
    pub to: String,
    pub amount: u128,
}

/// No verified config: a dev setup step on the deployment's token program.
pub fn prepare_token_transfer(
    d: &Deployment,
    req: &TokenTransferRequest,
    accounts: &Accounts<'_>,
) -> SdkResult<Prepared> {
    let (from, to) = (accounts.id(&req.from)?, accounts.id(&req.to)?);
    let p = plan_host::token_transfer(d.token_program, from, to, req.amount)?;
    Ok(Prepared::one(p, "transfer", CHECK_BALANCE))
}

/// The flags of `auction-cli dev oracle-publish` (and `dev keeper`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OraclePublishRequest {
    /// The price-source account (signs the new price account; one per deployment).
    pub price_source: String,
    /// Collateral per 1 stablecoin, RAY (1e27).
    pub price: u128,
    pub host_admin: String,
    pub engine_admin: String,
    /// First window tried; each publish takes the next free one (part of the account address).
    pub base_window_ms: u64,
}

/// One move of the price: new price account, host rotation, engine follow-up. Both admins and the
/// price are checked before anything is signed. `d` names the twap oracle program.
pub fn prepare_oracle_publish<C: Chain + ?Sized>(
    chain: &C,
    d: &Deployment,
    v: &VerifiedConfig,
    req: &OraclePublishRequest,
    accounts: &Accounts<'_>,
    now_ms: u64,
) -> SdkResult<Prepared> {
    let engine_admin = accounts.id(&req.engine_admin)?;
    require_engine_admin(v, engine_admin)?;
    let twap = d.twap_oracle_program.ok_or_else(|| {
        SdkError::Refused("the deployment file has no twap_oracle_program_id".into())
    })?;
    let r = Reader::new(chain, v.engine.program);
    let params: ProtocolParameters = r.host(
        &v.host.protocol_parameters,
        &v.host.program,
        "ProtocolParameters",
    )?;
    let host_admin = accounts.id(&req.host_admin)?;
    require_host_admin(&params, host_admin)?;
    let source = accounts.id(&req.price_source)?;
    let coll = v.coll(chain)?;
    let old = r.oracle(&coll.oracle_id)?;
    let (window, new_oracle) = oracle::free_oracle_window(chain, twap, source, req.base_window_ms)?;
    oracle::check_new_price(chain, v, new_oracle, source, req.price, now_ms).map_err(|e| {
        SdkError::Refused(format!(
            "refusing before signing: price {} would make the feed invalid for the engine ({e:?})",
            req.price
        ))
    })?;
    let (create, created) = plan_host::create_oracle_price_account(
        twap,
        source,
        params.stablecoin_definition_id,
        params.collateral_definition_id,
        req.price,
        window,
    )?;
    if created != new_oracle {
        return Err(SdkError::Refused(
            "oracle address derivation disagrees".into(),
        ));
    }
    let rotate = plan_host::set_market_price_oracle(&v.host, host_admin, new_oracle)?;
    let args = oracle::collateral_args_from_state(&coll)?;
    let follow = plan::set_collateral_config(v, engine_admin, new_oracle, &args)?;
    let step = |plan, what| Step {
        plan,
        what,
        check: CHECK_HOST,
    };
    Ok(Prepared {
        steps: vec![
            step(create, "create_oracle_price_account"),
            step(rotate, "set_market_price_oracle"),
            step(follow, "set_collateral_config"),
        ],
        facts: vec![
            fact("price", format!("{} -> {}", old.price, req.price)),
            fact("new oracle", new_oracle),
            fact("window ms", window),
        ],
        touched: Vec::new(),
    })
}
