//! The engine invariants the tests assert after every transaction (v0.2.4
//! `programs/integration_tests` invariants, design note §4.3 a), over every auction the world's
//! positions have had (found through their liquidation records, so nothing has to be tracked):
//!
//! - **R6 (as a floor, ruling R-5e):** each auction's vault holds at least
//!   `collateral_seized − keeper_reward − collateral_sold − leftover_claimed`. Donations into a
//!   host vault before a seizure land in the auction vault unbooked, so equality cannot hold.
//! - **Reservations:** the collateral actor's `type_reserved` and the config's `global_reserved`
//!   both equal the sum of `reserved_amount` over live (`Auctioning`) auctions (inv. 2/7).
//! - **F6:** every auction is `Auctioning` or `Settled`, never `AwaitingSeize`, and no engine
//!   scratch (`pending_read`, `pending`, `scratch`) survives a transaction.

use engine_core::state::{
    AuctionPhase, AuctionState, CollateralState, ConfigState, LiquidationRecordState,
};
use lee::Actor;

use crate::World;

/// Panics with the broken invariant's name unless every invariant holds in `w`.
pub fn check(w: &World) {
    let mut live_reserved: u128 = 0;
    for owner in &w.owners {
        let Some(record) = decoded(w, w.record(owner.position), LiquidationRecordState::decode)
        else {
            continue;
        };
        assert_eq!(record.position, owner.position, "record: position");
        for nonce in 0..record.count {
            let auction = decoded(w, w.auction(owner.position, nonce), AuctionState::decode)
                .unwrap_or_else(|| panic!("auction {nonce} of a counted record exists"));
            check_auction(w, &auction, owner.position, nonce);
            if auction.phase == AuctionPhase::Auctioning {
                live_reserved = live_reserved
                    .checked_add(auction.reserved_amount)
                    .expect("reservations fit u128");
            }
        }
    }
    if let Some(collateral) = decoded(w, w.collateral, CollateralState::decode) {
        assert_eq!(collateral.pending, None, "F6: collateral scratch cleared");
        assert_eq!(
            collateral.type_reserved, live_reserved,
            "reservations: type_reserved == sum over live auctions"
        );
    }
    if let Some(config) = decoded(w, w.config, ConfigState::decode) {
        assert_eq!(config.pending_read, None, "F6: config scratch cleared");
        assert_eq!(
            config.global_reserved, live_reserved,
            "reservations: global_reserved == sum over live auctions"
        );
    }
}

fn check_auction(w: &World, a: &AuctionState, position: lee::AccountId, nonce: u64) {
    assert_eq!(
        (a.config, a.position, a.nonce),
        (w.config.account_id, position, nonce),
        "auction identity"
    );
    assert!(
        matches!(a.phase, AuctionPhase::Auctioning | AuctionPhase::Settled),
        "F6: auction {nonce} is {:?}",
        a.phase
    );
    assert_eq!(a.scratch, None, "F6: auction scratch cleared");
    let booked = a
        .collateral_seized
        .checked_sub(a.keeper_reward)
        .and_then(|left| left.checked_sub(a.collateral_sold))
        .and_then(|left| left.checked_sub(a.leftover_claimed))
        .expect("R6: book never negative");
    let vault = w.vault(position, nonce).account_id;
    let held = w.collateral_balance(vault);
    assert!(
        held >= booked,
        "R6: vault holds {held} < booked {booked} (auction {nonce})"
    );
}

fn decoded<T>(w: &World, actor: Actor, decode: fn(&[u8]) -> Option<T>) -> Option<T> {
    w.chain.raw_state(actor).map(|bytes| {
        decode(&bytes).unwrap_or_else(|| panic!("state of {actor:?} has the wrong type"))
    })
}
