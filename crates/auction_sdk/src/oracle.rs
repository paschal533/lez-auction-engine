//! Localnet price publisher support (ADR-050). The localnet market oracle is a twap_oracle
//! `OraclePriceAccount` created at RAY scale; twap's own `PublishPrice` writes a Q64.64 TWAP of ticks
//! (the open oracle-scale question, spec §5.2). So the price moves by rotation: a new price account for
//! the same source at the next free window, then the host's `SetMarketPriceOracle`, then the engine's
//! `set_collateral_config` with unchanged parameters and the new oracle (ADR-028: the engine must
//! follow the host). Production uses a real publisher once the scale question is settled (M2).

use auction_engine_core::{args::CollateralArgs, state::CollateralConfigState};
use auction_engine_program::feed;
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::ProgramId,
};
use twap_oracle_core::OraclePriceAccount;

use crate::{
    chain::Chain,
    decode::Reader,
    error::{SdkError, SdkResult},
    verify::VerifiedConfig,
};

/// Window probes before `free_oracle_window` gives up (each probe is one account read).
pub const MAX_WINDOW_PROBES: u64 = 10_000;

/// The `set_collateral_config` arguments that reproduce `st` (the oracle is an account, passed apart).
pub fn collateral_args_from_state(st: &CollateralConfigState) -> SdkResult<CollateralArgs> {
    let mut schedule = Vec::with_capacity(st.schedule.len().saturating_mul(2));
    for p in &st.schedule {
        let elapsed = u32::try_from(p.elapsed_ms).map_err(|_| {
            SdkError::Encode(format!(
                "schedule point at {} ms does not fit the u32 argument",
                p.elapsed_ms
            ))
        })?;
        schedule.push(elapsed);
        schedule.push(u32::from(p.discount_bps));
    }
    Ok(CollateralArgs {
        oracle_scale: st.oracle_scale,
        max_oracle_age_ms: st.max_oracle_age_ms,
        max_conf_bps: u32::from(st.max_conf_bps),
        band_lo_bps: u32::from(st.band_lo_bps),
        band_hi_bps: u32::from(st.band_hi_bps),
        penalty_bps: u32::from(st.penalty_bps),
        tip: st.tip,
        chip_bps: u32::from(st.chip_bps),
        reward_cap: st.reward_cap,
        debt_floor: st.debt_floor,
        min_bid: st.min_bid,
        type_cap: st.type_cap,
        schedule,
        admin_paused: st.admin_paused,
    })
}

/// Runs the engine's own feed validation (`feed::observe` + `feed::require_live`, the `liquidate`/`bid`
/// path) on a hypothetical new oracle at `now_ms`, so a price the engine would reject (outside
/// `[R/2, 2R]`, zero) is refused before anything is signed. The band only clamps; it never refuses.
pub fn check_new_price<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    new_oracle: AccountId,
    price_source: AccountId,
    price: u128,
    now_ms: u64,
) -> SdkResult<()> {
    let r = Reader::new(chain, v.engine.program);
    let cfg = r.config(&v.config_id)?;
    let mut coll = r.coll_config(&v.coll_config_id())?;
    let current = r.oracle(&coll.oracle_id)?;
    let current_account = chain.account(&coll.oracle_id)?;
    let candidate = OraclePriceAccount {
        price,
        timestamp: now_ms,
        source_id: price_source,
        confidence_interval: 0,
        ..current
    };
    coll.oracle_id = new_oracle;
    let meta = AccountWithMetadata::new(
        Account {
            data: Data::from(&candidate),
            ..current_account
        },
        false,
        new_oracle,
    );
    let redemption =
        feed::redemption_price(&cfg, &r.meta(&v.host.redemption_price_state)?, now_ms)?;
    let obs = feed::observe(&coll, &meta, redemption, now_ms)?;
    feed::require_live(&coll, &obs)?;
    Ok(())
}

/// The first window `>= base` whose price account for `source` does not exist yet, and that account.
pub fn free_oracle_window<C: Chain + ?Sized>(
    chain: &C,
    twap: ProgramId,
    source: AccountId,
    base: u64,
) -> SdkResult<(u64, AccountId)> {
    for k in 0..MAX_WINDOW_PROBES {
        let window = base
            .checked_add(k)
            .ok_or_else(|| SdkError::Encode("oracle window overflows u64".into()))?;
        let id = twap_oracle_core::compute_oracle_price_account_pda(twap, source, window);
        if chain.account(&id)? == Account::default() {
            return Ok((window, id));
        }
    }
    Err(SdkError::Chain(format!(
        "no free oracle window in {base}..{base}+{MAX_WINDOW_PROBES} for this price source; use another --price-source"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{
        id,
        standard::{self, k},
    };
    use crate::verify::verify_config;

    fn verified(fx: &standard::Fixture) -> VerifiedConfig {
        verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies")
    }

    #[test]
    fn check_new_price_refuses_a_price_outside_the_validity_range() {
        let fx = standard::standard();
        let v = verified(&fx);
        let new = AccountId::new([0x74; 32]);
        check_new_price(
            &fx.chain,
            &v,
            new,
            id(k::CALLER),
            standard::R * 19 / 10,
            standard::T0,
        )
        .expect("1.9 R is inside [R/2, 2R]");
        let e = check_new_price(
            &fx.chain,
            &v,
            new,
            id(k::CALLER),
            standard::R * 3,
            standard::T0,
        )
        .expect_err("3 R would pause the engine");
        assert!(format!("{e:?}").contains("PriceFeedOutOfRange"), "{e:?}");
        let e = check_new_price(&fx.chain, &v, new, id(k::CALLER), 0, standard::T0)
            .expect_err("a zero price is invalid");
        assert!(format!("{e:?}").contains("PriceFeed"), "{e:?}");
    }

    #[test]
    fn collateral_args_round_trip_the_stored_config() {
        let fx = standard::standard();
        let v = verified(&fx);
        let st = v.coll(&fx.chain).expect("coll config");
        let args = collateral_args_from_state(&st).expect("args");
        assert_eq!(args.schedule.len(), 2 * st.schedule.len());
        assert_eq!(args.penalty_bps, u32::from(st.penalty_bps));
        assert_eq!(args.band_hi_bps, u32::from(st.band_hi_bps));
        assert_eq!(args.type_cap, st.type_cap);
        assert_eq!(args.admin_paused, st.admin_paused);
        args.validate(v.state.global_cap)
            .expect("the stored config is valid again");
    }

    #[test]
    fn free_oracle_window_skips_existing_price_accounts() {
        let fx = standard::standard();
        let source = id(k::CALLER);
        let taken =
            twap_oracle_core::compute_oracle_price_account_pda(standard::TWAP, source, 3_600_000);
        fx.chain.put_state(
            taken,
            standard::ORACLE_OWNER,
            &standard::oracle_state(standard::R, standard::T0, &fx.host),
        );
        let (w, id_) =
            free_oracle_window(&fx.chain, standard::TWAP, source, 3_600_000).expect("free");
        assert_eq!(w, 3_600_001);
        assert_eq!(
            id_,
            twap_oracle_core::compute_oracle_price_account_pda(standard::TWAP, source, 3_600_001)
        );
    }
}
