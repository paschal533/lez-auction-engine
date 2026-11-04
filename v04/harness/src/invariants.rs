//! The engine invariants the tests assert after every transaction (v0.2.4
//! `programs/integration_tests` invariants, design note §4.3 a), over every auction the world's
//! positions have had (found through their liquidation records, so nothing has to be tracked):
//!
//! - **R6 (as a floor, ruling R-5e):** each auction vault's collateral holding holds at least
//!   `collateral_seized − keeper_reward − collateral_sold − leftover_claimed`. Donations into a
//!   host vault before a seizure land in the auction vault unbooked, so equality cannot hold.
//! - **Reservations:** the collateral actor's `type_reserved` and the config's `global_reserved`
//!   both equal the sum of `reserved_amount` over live (`Auctioning`) auctions (inv. 2/7).
//! - **F6:** every auction is `Auctioning` or `Settled`, never `AwaitingSeize`, and no engine
//!   scratch (`pending_read`, `pending_genesis`, `pending`, `scratch`, the accounting's system
//!   `pending`) survives a transaction.
//! - **R-F3:** the config's recorded collateral type, if any, has its collateral actor.
//! - **Surplus (rows 10-12):** every started surplus auction exists with its close scratch
//!   cleared; one is live iff the accounting's `surplus_live`; a live one's vault holding holds
//!   at least `lot_left` (donations may add to it).
//! - **Debt (rows 13-15, ADR-024):** every started debt auction exists; one is live iff the
//!   accounting's `debt_live`, and then `debt_on_auction == lot_left`; `debt_on_auction == 0`
//!   when none is live; the reservation never exceeds `unbacked_debt`.

use engine_core::{
    actors::collateral_actor,
    state::{
        AccountingState, AuctionPhase, AuctionState, CollateralState, ConfigState,
        LiquidationRecordState,
    },
    system_state::{DebtAuctionState, SurplusAuctionState, SystemPhase},
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
    if let Some(accounting) = decoded(w, w.accounting, AccountingState::decode) {
        assert_eq!(accounting.pending, None, "F6: accounting scratch cleared");
        check_surplus(w, &accounting);
        check_debt(w, &accounting);
    }
    if let Some(config) = decoded(w, w.config, ConfigState::decode) {
        assert_eq!(config.pending_read, None, "F6: config scratch cleared");
        assert_eq!(config.pending_genesis, None, "F6: config genesis consumed");
        if let Some(def) = config.collateral_def {
            let recorded = collateral_actor(w.engine, w.config.account_id, def);
            assert!(
                decoded(w, recorded, CollateralState::decode).is_some(),
                "R-F3: the config's recorded collateral type exists"
            );
        }
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

/// Every surplus auction the accounting has started: its close scratch is cleared, a live one
/// exists iff `surplus_live` (at most one), and a live one's vault holds at least its lot.
fn check_surplus(w: &World, acc: &AccountingState) {
    let mut live = 0_u32;
    for nonce in 0..acc.surplus_nonce {
        let s = decoded(w, w.surplus(nonce), SurplusAuctionState::decode)
            .unwrap_or_else(|| panic!("surplus auction {nonce} of a counted nonce exists"));
        assert_eq!(
            (s.config, s.nonce),
            (w.config.account_id, nonce),
            "surplus identity"
        );
        assert_eq!(s.close, None, "F6: surplus {nonce} close scratch cleared");
        if s.phase == SystemPhase::Live {
            live = live.checked_add(1).expect("few auctions");
            let held = w.stable_balance(w.surplus_vault(nonce).account_id);
            assert!(
                held >= s.lot_left,
                "surplus vault holds {held} < lot {} (auction {nonce})",
                s.lot_left
            );
        }
    }
    assert!(live <= 1, "at most one live surplus auction");
    assert_eq!(
        live == 1,
        acc.surplus_live,
        "a live surplus auction iff surplus_live"
    );
}

/// Every debt auction the accounting has started exists; a live one exists iff `debt_live` (at
/// most one) and its lot is exactly the reservation (ADR-024 pairing); none live, none reserved.
fn check_debt(w: &World, acc: &AccountingState) {
    assert!(
        acc.debt_on_auction <= acc.unbacked_debt,
        "debt_on_auction {} > unbacked_debt {}",
        acc.debt_on_auction,
        acc.unbacked_debt
    );
    let mut live = Vec::new();
    for nonce in 0..acc.debt_nonce {
        let d = decoded(w, w.debt(nonce), DebtAuctionState::decode)
            .unwrap_or_else(|| panic!("debt auction {nonce} of a counted nonce exists"));
        assert_eq!(
            (d.config, d.nonce),
            (w.config.account_id, nonce),
            "debt identity"
        );
        if d.phase == SystemPhase::Live {
            live.push(d.lot_left);
        }
    }
    assert!(live.len() <= 1, "at most one live debt auction");
    assert_eq!(
        !live.is_empty(),
        acc.debt_live,
        "a live debt auction iff debt_live"
    );
    assert_eq!(
        acc.debt_on_auction,
        live.first().copied().unwrap_or(0),
        "debt_on_auction == the live debt auction's lot_left, else 0"
    );
}

fn decoded<T>(w: &World, actor: Actor, decode: fn(&[u8]) -> Option<T>) -> Option<T> {
    w.chain.raw_state(actor).map(|bytes| {
        decode(&bytes).unwrap_or_else(|| panic!("state of {actor:?} has the wrong type"))
    })
}
