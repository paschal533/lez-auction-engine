//! Security tests with an impostor bound as the engine's host (design note §4.3 c, guards 2 and
//! 4; Review Focus 1 and 5). The `impostor` guest (`harness::impostor`) stands in as the host:
//! it answers the Observe's `ReadState` like the mock host, and on `LiquidatePosition` either
//! pings an impostor "position" whose script reports `Seized`, or sends nothing at all.
//! `security.rs` holds the forged-message and rogue-config tests.

use auction_engine_params::EngineError;
use engine_core::{
    check::fail_text,
    state::{AuctionPhase, AuctionState, CollateralState, ConfigState},
};
use harness::{World, expect_revert, world::RAY};
use lee::{Actor, error::LeeError};
use lee_core::program::TransactionEvent;

const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
/// The chain time of the liquidation, ms.
const T: u64 = 1_000_000;

/// A world whose engine is bound to an impostor host `(host, Impostor)`, at `T` with the oracle
/// at 1 RAY (in band). The liquidated "position" is the impostor `(position, Impostor)`, the
/// actor an auction accepts `Seized` from. With `reporting`, the host pings that actor on
/// `LiquidatePosition` and its script reports `Seized { 500_000, 600_000 }` to auction 0, whose
/// vault was first funded with the 500_000; otherwise the host accepts `LiquidatePosition` and
/// replies nothing.
struct HostedWorld {
    w: World,
    host: Actor,
    position: Actor,
}

impl HostedWorld {
    fn new(reporting: bool) -> Self {
        let mut w = World::bare();
        w.set_now_ms(T);
        w.set_price(RAY);
        let config = w.config.account_id;
        let (position, host) = if reporting {
            let position = w.scripted_position("impostor-position", config, COLLATERAL, DEBT);
            (position, w.impostor_host("impostor-host", Some(position)))
        } else {
            let (_, position) = w.fresh_impostor("impostor-position");
            (position, w.impostor_host("impostor-host", None))
        };
        let admin_key = w.admin_key.clone();
        let bound = w.configure_with_host(w.admin, &admin_key, host);
        assert_eq!(bound, w.config);
        if reporting {
            w.fund_vault(config, position.account_id, COLLATERAL);
        }
        Self { w, host, position }
    }

    fn auction(&self) -> Actor {
        self.w.auction(self.position.account_id, 0)
    }

    fn vault(&self) -> Actor {
        self.w.vault(self.position.account_id, 0)
    }

    fn liquidate(&mut self) -> Result<Vec<TransactionEvent>, LeeError> {
        let config = self.w.config.account_id;
        self.w.liquidate_through(config, self.host, self.position)
    }
}

fn collateral_state(w: &World) -> CollateralState {
    w.chain
        .raw_state(w.collateral)
        .and_then(|bytes| CollateralState::decode(&bytes))
        .expect("a collateral state")
}

fn config_state(w: &World) -> ConfigState {
    w.chain
        .raw_state(w.config)
        .and_then(|bytes| ConfigState::decode(&bytes))
        .expect("a config state")
}

fn auction_state(w: &World, auction: Actor) -> Option<AuctionState> {
    w.chain
        .raw_state(auction)
        .map(|bytes| AuctionState::decode(&bytes).expect("an auction state"))
}

/// Review Focus 1 (guard 2): `Seized` is a reply, accepted only in `AwaitingSeize`. Once the
/// liquidation committed the auction `Auctioning`, a second `Seized` from exactly the sender the
/// auction expects, `(position, host program)`, is refused, whether it replays the first report or
/// reports a different seizure.
#[test]
fn seized_outside_awaiting_seize_reverts() {
    let mut h = HostedWorld::new(true);
    h.liquidate()
        .expect("the impostor host reports and the auction opens");
    let opened = auction_state(&h.w, h.auction()).expect("an auction");
    assert_eq!(opened.phase, AuctionPhase::Auctioning);
    // The reported seizure is booked at P = 1: reward `min(5 + 600, 50) × 1 = 50`.
    assert_eq!(
        (
            opened.collateral_seized,
            opened.keeper_reward,
            opened.collateral_left
        ),
        (COLLATERAL, 50, COLLATERAL - 50)
    );
    assert_eq!(opened.scratch, None);
    let vault = h.w.collateral_balance(h.vault().account_id);
    assert_eq!(vault, opened.collateral_left);
    let reserved = |w: &World| {
        (
            collateral_state(w).type_reserved,
            config_state(w).global_reserved,
        )
    };
    let reserved_before = reserved(&h.w);
    assert_eq!(
        reserved_before,
        (opened.reserved_amount, opened.reserved_amount)
    );

    let (position, auction) = (h.position, h.auction());
    for collateral in [COLLATERAL, 1] {
        let seized = World::impostor_seized(position.account_id, collateral, DEBT);
        expect_revert(
            h.w.speak(position, auction, &seized),
            &fail_text(EngineError::WrongAccount("auction phase")),
        );
    }
    assert_eq!(auction_state(&h.w, auction), Some(opened.clone()));
    assert_eq!(h.w.collateral_balance(h.vault().account_id), vault);
    assert_eq!(reserved(&h.w), reserved_before);
}

/// Review Focus 5 (guard 4): a bound host that accepts `LiquidatePosition` and never reports.
/// The config's `Finish` sentinel finds the auction still `AwaitingSeize` and panics, so the
/// whole liquidation reverts: no auction, no record, no reservation, no reward, no vault.
#[test]
fn host_that_never_replies_leaves_no_auction() {
    let mut h = HostedWorld::new(false);
    let p = h.position.account_id;
    let collateral_before = h.w.chain.raw_state(h.w.collateral);
    let config_before = h.w.chain.raw_state(h.w.config);
    expect_revert(h.liquidate(), &fail_text(EngineError::NotAuctioning));
    assert_eq!(h.w.chain.raw_state(h.auction()), None, "no auction");
    assert_eq!(h.w.chain.raw_state(h.w.record(p)), None, "no record bump");
    assert_eq!(h.w.chain.raw_state(h.vault()), None, "no vault");
    assert_eq!(
        h.w.chain.raw_state(Actor::new(h.w.keeper, h.w.token)),
        None,
        "no reward"
    );
    assert_eq!(h.w.chain.raw_state(h.w.collateral), collateral_before);
    assert_eq!(h.w.chain.raw_state(h.w.config), config_before);
    let coll = collateral_state(&h.w);
    assert_eq!((coll.type_reserved, coll.pending), (0, None));
}
