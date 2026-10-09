//! Handler tests of `system::ref_price` and `system::protocol_token_status` (moved from surplus_tests.rs).

use auction_engine_core::{
    pda::{mint_authority_pda, SLOT_PROTOCOL_TOKEN, SLOT_REF_ORACLE},
    state::*,
    EngineError,
};
use lee_core::account::{Account, AccountId, AccountWithMetadata, Data};
use token_core::TokenDefinition;

use crate::{
    admin_tests::{config_state, fresh, meta, ENGINE, ONE, TOKEN},
    collateral_tests::cfg_id,
    system::{protocol_token_status, ref_price, TokenStatus},
};

use crate::surplus_tests::*;

// ---- ref_price / protocol_token_status ----

pub(crate) fn ref_with(
    cfg: &EngineConfigState,
    slot: &AccountWithMetadata,
    now: u64,
) -> Result<u128, EngineError> {
    ref_price(&ENGINE, &cfg_id(), cfg, slot, now)
}

#[test]
pub(crate) fn ref_price_is_the_admin_price_without_an_oracle_and_pins_the_placeholder() {
    let cfg = protocol_config(false);
    assert_eq!(
        ref_with(&cfg, &placeholder(SLOT_REF_ORACLE), NOW),
        Ok(ADMIN_REF)
    );
    assert_eq!(
        ref_with(&cfg, &fresh(FOREIGN), NOW),
        Err(EngineError::WrongAccount("unused slot"))
    );
    // The placeholder of the other slot is not accepted either.
    assert_eq!(
        ref_with(&cfg, &placeholder(SLOT_PROTOCOL_TOKEN), NOW),
        Err(EngineError::WrongAccount("unused slot"))
    );
}

#[test]
pub(crate) fn ref_price_pins_the_configured_oracle_and_rejects_invalid_readings() {
    let cfg = protocol_config(true);
    let mut wrong = oracle_account(2 * ONE, NOW);
    wrong.account_id = FOREIGN;
    assert_eq!(
        ref_with(&cfg, &wrong, NOW),
        Err(EngineError::WrongAccount("slot"))
    );
    // Undecodable data and a price outside [admin/2, 2 admin] both fall back to the admin price.
    let garbage = meta(REF_ORACLE, Account::default(), false);
    assert_eq!(ref_with(&cfg, &garbage, NOW), Ok(ADMIN_REF));
    assert_eq!(
        ref_with(&cfg, &oracle_account(5 * ONE, NOW), NOW),
        Ok(ADMIN_REF)
    );
    assert_eq!(
        ref_with(&cfg, &oracle_account(ONE / 2, NOW), NOW),
        Ok(ADMIN_REF)
    );
    assert_eq!(
        ref_with(&cfg, &oracle_account(24 * ONE / 10, NOW), NOW),
        Ok(24 * ONE / 10)
    );
    assert_eq!(
        ref_with(
            &cfg,
            &oracle_account(3 * ONE, NOW - cfg.ref_oracle_max_age_ms),
            NOW
        ),
        Ok(24 * ONE / 10),
        "age exactly at the maximum is still valid"
    );
}

#[test]
pub(crate) fn protocol_token_status_reports_id_owner_type_and_mint_authority() {
    let status = |cfg: &EngineConfigState, slot: &AccountWithMetadata| {
        protocol_token_status(&ENGINE, &cfg_id(), cfg, slot)
    };
    let cfg = protocol_config(false);
    let mint_authority = mint_authority_pda(&ENGINE, &cfg_id());
    let def = |authority: Option<AccountId>, owner: [u32; 8]| {
        let mut d = protocol_definition();
        d.account.program_owner = owner;
        d.account.data = Data::from(&TokenDefinition::Fungible {
            name: String::from("MOCK"),
            total_supply: 1,
            metadata_id: None,
            authority,
        });
        d
    };
    let status_of = |configured, token_ok, mint_authority_ok| {
        Ok(TokenStatus {
            configured,
            token_ok,
            mint_authority_ok,
        })
    };
    // Not configured: the placeholder is required and nothing else is reported.
    assert_eq!(
        status(&config_state(), &placeholder(SLOT_PROTOCOL_TOKEN)),
        status_of(false, false, false)
    );
    assert_eq!(
        status(&config_state(), &protocol_definition()),
        Err(EngineError::WrongAccount("unused slot"))
    );
    // Configured and correct.
    assert_eq!(
        status(&cfg, &def(Some(mint_authority), TOKEN)),
        status_of(true, true, true)
    );
    // Wrong or missing mint authority.
    assert_eq!(
        status(&cfg, &def(Some(FOREIGN), TOKEN)),
        status_of(true, true, false)
    );
    assert_eq!(
        status(&cfg, &def(None, TOKEN)),
        status_of(true, true, false)
    );
    // Foreign owner, non-fungible, and a swapped account id.
    assert_eq!(
        status(&cfg, &def(Some(mint_authority), OTHER_PROGRAM)),
        status_of(true, false, false)
    );
    let mut nft = protocol_definition();
    nft.account.data = Data::from(&TokenDefinition::NonFungible {
        name: String::from("N"),
        printable_supply: 1,
        metadata_id: FOREIGN,
    });
    assert_eq!(status(&cfg, &nft), status_of(true, false, false));
    let mut swapped = def(Some(mint_authority), TOKEN);
    swapped.account_id = FOREIGN;
    assert_eq!(
        status(&cfg, &swapped),
        Err(EngineError::WrongAccount("slot"))
    );
}
