//! ADR-050: the localnet publisher's rotation on the real guests (host `SetMarketPriceOracle`, then the
//! engine's `set_collateral_config` from `collateral_args_from_state`), exactly the plans the CLI sends.

mod common;

use auction_engine_core::{state::CollateralConfigState, Instruction as Ix};
use auction_sdk::{addresses::HostAddresses, oracle::collateral_args_from_state, plan, plan_host};
use common::*;
use lee_core::account::AccountId;

fn with_new_oracle(accounts: Vec<AccountId>, new_oracle: AccountId) -> Vec<AccountId> {
    accounts
        .into_iter()
        .map(|a| if a == oracle() { new_oracle } else { a })
        .collect()
}

#[test]
fn rotating_the_oracle_mid_auction_keeps_bids_on_the_new_feed() {
    let (mut w, pos) = liquidated_world();
    w.bid(pos, 100_000, 0).expect("partial bid on the old feed");
    let new_oracle = AccountId::new([0x73; 32]);
    w.put_oracle_at(new_oracle, R * 11 / 10, w.now, 0);
    let host = HostAddresses::new(host_program());
    let rotate =
        plan_host::set_market_price_oracle(&host, id(k::HOST_ADMIN), new_oracle).expect("plan");
    w.run_plan(&rotate, &[k::HOST_ADMIN])
        .expect("the host rotates");
    let v = w.sdk_config();
    let before: CollateralConfigState = w.read(coll_config());
    let args = collateral_args_from_state(&before).expect("args");
    let follow =
        plan::set_collateral_config(&v, id(k::ENGINE_ADMIN), new_oracle, &args).expect("plan");
    w.run_plan(&follow, &[k::ENGINE_ADMIN])
        .expect("the engine follows the host");
    let after: CollateralConfigState = w.read(coll_config());
    assert_eq!(after.oracle_id, new_oracle);
    assert_eq!(
        collateral_args_from_state(&after).expect("args"),
        args,
        "parameters unchanged"
    );
    assert_eq!(
        after.type_reserved, before.type_reserved,
        "the live reservation is untouched"
    );
    let old = w.bid_accounts(pos, 0, k::BIDDER_STABLE, k::BIDDER_COLL, false);
    assert_err(
        w.engine(
            old.clone(),
            &[k::BIDDER_STABLE],
            Ix::Bid {
                coin_in_max: 50_000,
                min_collateral_out: 0,
            },
        ),
        "OracleMismatch",
    );
    w.engine(
        with_new_oracle(old, new_oracle),
        &[k::BIDDER_STABLE],
        Ix::Bid {
            coin_in_max: 50_000,
            min_collateral_out: 0,
        },
    )
    .expect("a bid on the new feed fills");
}
