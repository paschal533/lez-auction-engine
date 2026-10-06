//! `bid` / `bid_with_settlement` (spec §5.6 row 3). Debt first: Burn the debt part from the bidder →
//! Transfer the penalty part to the buffer → Transfer collateral to the bidder (ADR-010). Chain end
//! (target reached or lot exhausted) needs the Accounting account (ADR-023, ADR-029). A lot-emptying
//! fill pays the whole vault balance, sweeping donations (ADR-033).

use auction_core::{
    auction::{apply_bid, quote_bid, BidRequest},
    price::bid_price,
};
use auction_engine_core::{pda, state::*, EngineError};
use lee_core::{account::AccountWithMetadata, program::ProgramId};

use crate::{accounts::*, feed, token_calls, EngineResult};

pub struct BidAccounts {
    pub bidder_stable_holding: AccountWithMetadata,
    pub bidder_collateral_holding: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub coll_config: AccountWithMetadata,
    pub auction: AccountWithMetadata,
    pub auction_vault: AccountWithMetadata,
    pub buffer: AccountWithMetadata,
    pub stablecoin_definition: AccountWithMetadata,
    pub redemption_price_state: AccountWithMetadata,
    pub oracle: AccountWithMetadata,
    pub clock: AccountWithMetadata,
    /// `Some` only for `bid_with_settlement`.
    pub accounting: Option<AccountWithMetadata>,
}

pub fn bid(
    program_id: ProgramId,
    a: BidAccounts,
    coin_in_max: u128,
    min_collateral_out: u128,
) -> EngineResult {
    if !a.bidder_stable_holding.is_authorized {
        return Err(EngineError::WrongAccount("bidder_stable_holding"));
    }
    let now = read_clock(&a.clock)?;
    let mut cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let coll = load_coll(&program_id, &config_id, &cfg, &a.coll_config)?;
    let st = load_auction(&program_id, &config_id, &a.auction)?;
    let current = st.to_core()?;

    // Observation precedes every t_eff use (ADR-024).
    let r = feed::redemption_price(&cfg, &a.redemption_price_state, now)?;
    let obs = feed::observe(&coll, &a.oracle, r, now)?;
    let m = feed::require_live(&coll, &obs)?;
    let mut coll = coll.with_pause_clock(obs.clock);
    let params = coll.collateral_params()?;
    let p = bid_price(m, r, &coll.feed_params()?);
    let t_eff =
        obs.clock
            .effective_elapsed(current.start_ms, current.type_paused_total_at_start, now);
    let fill = quote_bid(
        &current,
        &BidRequest {
            coin_in_max,
            min_collateral_out,
        },
        p,
        t_eff,
        params.min_bid,
        params.chost()?,
    )?;
    let (next, end) = apply_bid(&current, &fill);

    let token = cfg.token_program_id;
    expect_id(
        &a.stablecoin_definition,
        &cfg.stablecoin_definition_id,
        "stablecoin_definition",
    )?;
    expect_id(
        &a.buffer,
        &pda::buffer_pda(&program_id, &config_id),
        "buffer",
    )?;
    fungible_balance(&a.buffer, &token, &cfg.stablecoin_definition_id)?;
    expect_id(
        &a.auction_vault,
        &pda::vault_pda(&program_id, &a.auction.account_id),
        "auction_vault",
    )?;
    let vault_balance = fungible_balance(&a.auction_vault, &token, &cfg.collateral_definition_id)?;
    if vault_balance < fill.collateral_out {
        return Err(EngineError::VaultBelowBook);
    }
    let stable_balance = fungible_balance(
        &a.bidder_stable_holding,
        &token,
        &cfg.stablecoin_definition_id,
    )?;
    if stable_balance < fill.charged {
        return Err(EngineError::InsufficientBalance);
    }
    check_recipient(
        &a.bidder_collateral_holding,
        &token,
        &cfg.collateral_definition_id,
    )?;

    // Chain end: fold the cumulative raise and book the shortfall once, release the reservation.
    let accounting_post = match (end, a.accounting.as_ref()) {
        (Some(_), None) => return Err(EngineError::ChainEndAccountsMissing),
        (Some(e), Some(acc_meta)) => {
            let acc = load_accounting(&program_id, &config_id, acc_meta)?;
            coll.type_reserved = coll
                .type_reserved
                .checked_sub(e.release)
                .ok_or(EngineError::Math)?;
            cfg.global_reserved = cfg
                .global_reserved
                .checked_sub(e.release)
                .ok_or(EngineError::Math)?;
            Some(write(
                acc_meta,
                &AccountingState::with_core(&acc, &acc.core().record_settled(&next, &e)),
            ))
        }
        (None, Some(acc_meta)) => {
            load_accounting(&program_id, &config_id, acc_meta)?;
            Some(echo(acc_meta))
        }
        (None, None) => None,
    };

    let lot_empties = fill.collateral_out == current.collateral_left;
    let collateral_paid = if lot_empties {
        vault_balance
    } else {
        fill.collateral_out
    };
    let stable_after_burn = stable_balance
        .checked_sub(fill.debt_part)
        .ok_or(EngineError::Math)?;

    let mut calls = Vec::with_capacity(3);
    if fill.debt_part > 0 {
        calls.push(token_calls::burn(
            token,
            a.stablecoin_definition.clone(),
            a.bidder_stable_holding.clone(),
            fill.debt_part,
            vec![],
        ));
    }
    if fill.penalty_part > 0 {
        calls.push(token_calls::transfer(
            token,
            token_calls::with_balance(
                &a.bidder_stable_holding,
                cfg.stablecoin_definition_id,
                stable_after_burn,
            ),
            a.buffer.clone(),
            fill.penalty_part,
            vec![],
        ));
    }
    calls.push(token_calls::transfer(
        token,
        token_calls::authorized(&a.auction_vault),
        a.bidder_collateral_holding.clone(),
        collateral_paid,
        vec![pda::vault_seed(&a.auction.account_id)],
    ));

    let mut post = vec![
        echo(&a.bidder_stable_holding),
        echo(&a.bidder_collateral_holding),
        write(&a.config, &cfg),
        write(&a.coll_config, &coll),
        write(&a.auction, &AuctionState::from_core(&st, &next)),
        echo(&a.auction_vault),
        echo(&a.buffer),
        echo(&a.stablecoin_definition),
        echo(&a.redemption_price_state),
        echo(&a.oracle),
        echo(&a.clock),
    ];
    if let Some(p) = accounting_post {
        post.push(p);
    }
    Ok((post, calls))
}
