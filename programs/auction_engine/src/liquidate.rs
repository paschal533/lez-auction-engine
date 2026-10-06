//! `liquidate(nonce)` (spec §5.6 row 2, §5.9): observe → quote (caps, reward) → record nonce →
//! open the auction and reserve → chained: init vault, host hook (sweeps the whole host vault into
//! the auction vault), keeper reward. Hook arguments come only from the position read here
//! (ADR-019 invariant iii); `ENGINE_AUTH` seeds go only to `cfg.host_program_id` (invariant i).

use auction_core::{
    auction::Auction,
    liquidation::{quote_liquidation, CapState, LiquidationInput},
    math::mul_div_ceil,
    price::bid_price,
    units::RAY,
};
use auction_engine_core::{pda, state::*, EngineError};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::{
    compute_liquidation_engine_state_pda, compute_position_pda, compute_position_vault_pda,
    compute_stability_fee_accumulator_pda, math::compute_current_accumulated_rate_wide, Position,
    ProtocolParameters, StabilityFeeAccumulator,
};

use crate::{accounts::*, feed, token_calls, EngineResult};

pub struct LiquidateAccounts {
    pub keeper: AccountWithMetadata,
    pub keeper_collateral_holding: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub coll_config: AccountWithMetadata,
    pub position: AccountWithMetadata,
    pub record: AccountWithMetadata,
    pub auction: AccountWithMetadata,
    pub auction_vault: AccountWithMetadata,
    pub engine_authority: AccountWithMetadata,
    pub liquidation_engine_state: AccountWithMetadata,
    pub host_vault: AccountWithMetadata,
    pub collateral_definition: AccountWithMetadata,
    pub stability_fee_accumulator: AccountWithMetadata,
    pub redemption_price_state: AccountWithMetadata,
    pub oracle: AccountWithMetadata,
    pub protocol_parameters: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

fn host_owned(acc: &AccountWithMetadata, cfg: &EngineConfigState) -> Result<(), EngineError> {
    if acc.account.program_owner == cfg.host_program_id {
        Ok(())
    } else {
        Err(EngineError::HostMismatch)
    }
}

/// ProtocolParameters, accumulator (→ current accumulated rate) and the hook state, all checked
/// against the bound host (confused-deputy invariant iii).
fn load_host(
    cfg: &EngineConfigState,
    a: &LiquidateAccounts,
    now: u64,
) -> Result<(ProtocolParameters, u128), EngineError> {
    expect_id(
        &a.protocol_parameters,
        &cfg.host_protocol_parameters_id,
        "protocol_parameters",
    )?;
    host_owned(&a.protocol_parameters, cfg)?;
    let params = ProtocolParameters::try_from(&a.protocol_parameters.account.data)
        .map_err(|_| EngineError::HostMismatch)?;
    expect_id(
        &a.stability_fee_accumulator,
        &compute_stability_fee_accumulator_pda(cfg.host_program_id),
        "stability_fee_accumulator",
    )?;
    host_owned(&a.stability_fee_accumulator, cfg)?;
    let acc = StabilityFeeAccumulator::try_from(&a.stability_fee_accumulator.account.data)
        .map_err(|_| EngineError::HostMismatch)?;
    expect_id(
        &a.liquidation_engine_state,
        &compute_liquidation_engine_state_pda(cfg.host_program_id),
        "liquidation_engine_state",
    )?;
    let rate = feed::u512_to_u128(compute_current_accumulated_rate_wide(
        acc.accumulated_rate_at_last_accrual,
        params.stability_fee_per_millisecond,
        acc.last_accrued_at,
        now,
    ))?;
    Ok((params, rate))
}

fn load_position(cfg: &EngineConfigState, a: &LiquidateAccounts) -> Result<Position, EngineError> {
    host_owned(&a.position, cfg)?;
    let pos = Position::try_from(&a.position.account.data)
        .map_err(|_| EngineError::WrongAccount("position"))?;
    if a.position.account_id
        != compute_position_pda(
            cfg.host_program_id,
            pos.owner_account_id,
            pos.position_nonce,
        )
    {
        return Err(EngineError::WrongAccount("position"));
    }
    expect_id(
        &a.host_vault,
        &compute_position_vault_pda(cfg.host_program_id, a.position.account_id),
        "host_vault",
    )?;
    if pos.vault_account_id != a.host_vault.account_id {
        return Err(EngineError::WrongAccount("host_vault"));
    }
    Ok(pos)
}

/// `LiquidationRecord` created on first use; `nonce == count`, then `count += 1` (ADR-004, ADR-020:
/// only auction creation increments the counter).
fn bump_record(
    program_id: &ProgramId,
    config_id: &AccountId,
    position_id: &AccountId,
    record: &AccountWithMetadata,
    nonce: u64,
) -> Result<AccountPostState, EngineError> {
    expect_id(
        record,
        &pda::liqrec_pda(program_id, config_id, position_id),
        "record",
    )?;
    let current = if record.account == Account::default() {
        LiquidationRecordState {
            tag: TAG_LIQREC,
            position: *position_id,
            count: 0,
        }
    } else {
        if record.account.program_owner != *program_id {
            return Err(EngineError::WrongAccount("record"));
        }
        let s = LiquidationRecordState::try_from(&record.account.data)
            .map_err(|_| EngineError::WrongAccount("record"))?;
        if s.position != *position_id {
            return Err(EngineError::WrongAccount("record"));
        }
        s
    };
    if nonce != current.count {
        return Err(EngineError::NonceMismatch);
    }
    let next = LiquidationRecordState {
        count: current.count.checked_add(1).ok_or(EngineError::Math)?,
        ..current
    };
    Ok(write_or_create(
        record,
        &next,
        pda::liqrec_seed(config_id, position_id),
    ))
}

pub fn liquidate(program_id: ProgramId, a: LiquidateAccounts, nonce: u64) -> EngineResult {
    if !a.keeper.is_authorized {
        return Err(EngineError::WrongAccount("keeper"));
    }
    let now = read_clock(&a.clock)?;
    let mut cfg = load_config(&program_id, &a.config)?;
    let config_id = a.config.account_id;
    let coll = load_coll(&program_id, &config_id, &cfg, &a.coll_config)?;
    let (host_params, acc_rate) = load_host(&cfg, &a, now)?;
    if coll.oracle_id != host_params.market_price_oracle_id {
        return Err(EngineError::OracleMismatch); // ADR-028
    }
    let r = feed::redemption_price(&cfg, &a.redemption_price_state, now)?;
    let obs = feed::observe(&coll, &a.oracle, r, now)?;
    let m = feed::require_live(&coll, &obs)?;
    let mut coll = coll.with_pause_clock(obs.clock);

    let pos = load_position(&cfg, &a)?;
    let c = fungible_balance(
        &a.host_vault,
        &cfg.token_program_id,
        &cfg.collateral_definition_id,
    )?;
    let d = mul_div_ceil(pos.normalized_debt_amount, acc_rate, RAY)?; // nominal debt, owed → up
    let p = bid_price(m, r, &coll.feed_params()?);
    let params = coll.collateral_params()?;
    let caps = CapState {
        type_cap: coll.type_cap,
        type_reserved: coll.type_reserved,
        global_cap: cfg.global_cap,
        global_reserved: cfg.global_reserved,
    };
    let q = quote_liquidation(
        &LiquidationInput {
            position_collateral: pos.collateral_amount,
            vault_balance: c,
            nominal_debt: d,
            price_ray: p,
        },
        &params,
        &caps,
    )?;

    let record_post = bump_record(
        &program_id,
        &config_id,
        &a.position.account_id,
        &a.record,
        nonce,
    )?;
    expect_id(
        &a.auction,
        &pda::auction_pda(&program_id, &config_id, &a.position.account_id, nonce),
        "auction",
    )?;
    expect_default(&a.auction, "auction")?;
    expect_id(
        &a.auction_vault,
        &pda::vault_pda(&program_id, &a.auction.account_id),
        "auction_vault",
    )?;
    expect_default(&a.auction_vault, "auction_vault")?;
    expect_id(
        &a.engine_authority,
        &pda::engine_authority_pda(&program_id, &config_id),
        "engine_authority",
    )?;
    expect_id(
        &a.collateral_definition,
        &cfg.collateral_definition_id,
        "collateral_definition",
    )?;
    if q.keeper_reward > 0 {
        check_recipient(
            &a.keeper_collateral_holding,
            &cfg.token_program_id,
            &cfg.collateral_definition_id,
        )?;
    }

    // Effects.
    let opened = Auction::open(&q, now, params.schedule, coll.paused_total_ms);
    let identity = AuctionState {
        tag: TAG_AUCTION,
        config: config_id,
        position: a.position.account_id,
        nonce,
        owner: pos.owner_account_id,
        keeper: a.keeper.account_id,
        settled: false,
        collateral_seized: 0,
        collateral_sold: 0,
        collateral_left: 0,
        keeper_reward: 0,
        leftover_claimable: 0,
        leftover_claimed: 0,
        debt_principal: 0,
        amount_to_raise: 0,
        reserved_amount: 0,
        debt_raised: 0,
        penalty_raised: 0,
        start_ms: 0,
        schedule: vec![],
        type_paused_total_at_start: 0,
    };
    let auction_state = AuctionState::from_core(&identity, &opened);
    coll.type_reserved = coll
        .type_reserved
        .checked_add(q.reserve)
        .ok_or(EngineError::Math)?;
    cfg.global_reserved = cfg
        .global_reserved
        .checked_add(q.reserve)
        .ok_or(EngineError::Math)?;

    // Chained calls (Shared Reference R3).
    let token = cfg.token_program_id;
    let init_vault = token_calls::initialize_account(
        token,
        a.collateral_definition.clone(),
        token_calls::authorized(&a.auction_vault),
        vec![pda::vault_seed(&a.auction.account_id)],
    );
    let custody = token_calls::post_init(&a.auction_vault, token, cfg.collateral_definition_id);
    let hook = ChainedCall::new(
        cfg.host_program_id,
        vec![
            token_calls::authorized(&a.engine_authority),
            a.liquidation_engine_state.clone(),
            a.position.clone(),
            a.host_vault.clone(),
            custody.clone(),
            a.stability_fee_accumulator.clone(),
            a.redemption_price_state.clone(),
            a.oracle.clone(),
            a.protocol_parameters.clone(),
            a.clock.clone(),
        ],
        &stablecoin_core::Instruction::LiquidatePosition {
            collateral_to_seize: pos.collateral_amount,
            normalized_debt_to_cover: pos.normalized_debt_amount,
        },
    )
    .with_pda_seeds(vec![pda::engine_authority_seed(&config_id)]);
    let mut calls = vec![init_vault, hook];
    if q.keeper_reward > 0 {
        // Full seizure: the hook moved the whole host vault balance `c` into custody.
        let swept = token_calls::authorized(&token_calls::with_balance(
            &custody,
            cfg.collateral_definition_id,
            c,
        ));
        calls.push(token_calls::transfer(
            token,
            swept,
            a.keeper_collateral_holding.clone(),
            q.keeper_reward,
            vec![pda::vault_seed(&a.auction.account_id)],
        ));
    }
    let post = vec![
        echo(&a.keeper),
        echo(&a.keeper_collateral_holding),
        write(&a.config, &cfg),
        write(&a.coll_config, &coll),
        echo(&a.position),
        record_post,
        create(
            &a.auction,
            &auction_state,
            pda::auction_seed(&config_id, &a.position.account_id, nonce),
        ),
        echo(&a.auction_vault),
        echo(&a.engine_authority),
        echo(&a.liquidation_engine_state),
        echo(&a.host_vault),
        echo(&a.collateral_definition),
        echo(&a.stability_fee_accumulator),
        echo(&a.redemption_price_state),
        echo(&a.oracle),
        echo(&a.protocol_parameters),
        echo(&a.clock),
    ];
    Ok((post, calls))
}
