//! F4/F5 shared helpers (spec §5.11, ADR-017, ADR-031): the protocol-token reference price and the
//! protocol token's status. Optional accounts are pinned to their configured id or the placeholder
//! (ADR-030), so a caller can never substitute an account that changes a decision.

use auction_core::{
    error::PauseReason,
    price::{
        effective_ref_price, validate_feed, FeedParams, OracleReading, OracleScale, MAX_CONF_BPS,
    },
};
use auction_engine_core::{
    pda::{self, SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE},
    state::EngineConfigState,
    EngineError,
};
use lee_core::{
    account::{AccountId, AccountWithMetadata},
    program::ProgramId,
};
use token_core::TokenDefinition;
use twap_oracle_core::OraclePriceAccount;

use crate::accounts::expect_slot;

/// Admin price; a configured oracle that is valid (age, confidence, within [admin/2, 2·admin]) overrides
/// it, clamped to ±20% of the admin price. An invalid oracle falls back to the admin price.
pub fn ref_price(
    program_id: &ProgramId,
    config_id: &AccountId,
    cfg: &EngineConfigState,
    slot: &AccountWithMetadata,
    now: u64,
) -> Result<u128, EngineError> {
    expect_slot(
        program_id,
        config_id,
        SLOT_REF_ORACLE,
        cfg.protocol_token_oracle_id,
        slot,
    )?;
    if cfg.protocol_token_oracle_id.is_none() {
        return Ok(cfg.protocol_token_ref_price);
    }
    let params = FeedParams {
        scale: OracleScale::Ray,
        max_age_ms: cfg.ref_oracle_max_age_ms,
        max_conf_bps: MAX_CONF_BPS,
        band_lo_bps: 10_000,
        band_hi_bps: 10_000,
    };
    let validity = match OraclePriceAccount::try_from(&slot.account.data) {
        Ok(o) => validate_feed(
            &OracleReading {
                price_raw: o.price,
                confidence_raw: o.confidence_interval,
                timestamp_ms: o.timestamp,
            },
            &params,
            cfg.protocol_token_ref_price,
            now,
        ),
        Err(_) => Err(PauseReason::OutOfRange),
    };
    Ok(effective_ref_price(
        cfg.protocol_token_ref_price,
        Some(validity),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenStatus {
    pub configured: bool,
    /// The configured definition exists, is owned by the config's token program and is fungible.
    pub token_ok: bool,
    /// Its mint authority is this config's `["MINT_AUTH", config]` PDA.
    pub mint_authority_ok: bool,
}

/// Pins the slot to the configured protocol-token id (or the placeholder) and reports its status.
pub fn protocol_token_status(
    program_id: &ProgramId,
    config_id: &AccountId,
    cfg: &EngineConfigState,
    slot: &AccountWithMetadata,
) -> Result<TokenStatus, EngineError> {
    expect_slot(
        program_id,
        config_id,
        SLOT_PROTOCOL_TOKEN,
        cfg.protocol_token,
        slot,
    )?;
    let not_ok = |configured| TokenStatus {
        configured,
        token_ok: false,
        mint_authority_ok: false,
    };
    if cfg.protocol_token.is_none() {
        return Ok(not_ok(false));
    }
    if slot.account.program_owner != cfg.token_program_id {
        return Ok(not_ok(true));
    }
    match TokenDefinition::try_from(&slot.account.data) {
        Ok(TokenDefinition::Fungible { authority, .. }) => Ok(TokenStatus {
            configured: true,
            token_ok: true,
            mint_authority_ok: authority == Some(pda::mint_authority_pda(program_id, config_id)),
        }),
        _ => Ok(not_ok(true)),
    }
}
