//! `observe_feed`: permissionless pause observation (spec §5.7, ADR-032). Never fails on a bad feed.

use auction_engine_core::EngineError;
use lee_core::{account::AccountWithMetadata, program::ProgramId};

use crate::{accounts::*, feed, EngineResult};

pub struct ObserveFeedAccounts {
    pub caller: AccountWithMetadata,
    pub config: AccountWithMetadata,
    pub coll_config: AccountWithMetadata,
    pub redemption_price_state: AccountWithMetadata,
    pub oracle: AccountWithMetadata,
    pub clock: AccountWithMetadata,
}

pub fn observe_feed(program_id: ProgramId, a: ObserveFeedAccounts) -> EngineResult {
    if !a.caller.is_authorized {
        return Err(EngineError::WrongAccount("caller"));
    }
    let now = read_clock(&a.clock)?;
    let cfg = load_config(&program_id, &a.config)?;
    let coll = load_coll(&program_id, &a.config.account_id, &cfg, &a.coll_config)?;
    let r = feed::redemption_price(&cfg, &a.redemption_price_state, now)?;
    let obs = feed::observe(&coll, &a.oracle, r, now)?;
    let next = coll.with_pause_clock(obs.clock);
    Ok((
        vec![
            echo(&a.caller),
            echo(&a.config),
            write(&a.coll_config, &next),
            echo(&a.redemption_price_state),
            echo(&a.oracle),
            echo(&a.clock),
        ],
        vec![],
    ))
}
