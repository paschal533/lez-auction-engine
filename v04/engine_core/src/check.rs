//! Checks every handler starts with (design note §3: compare whole actors, never bare ids).

use auction_engine_params::EngineError;
use lee_core::{account::Actor, program::ReceiveInput};

use crate::state::{AuctionPhase, AuctionState};

/// The v0.2.4 failure text: `Program error [6000+code]: Program error {code}: <Name>`.
#[must_use]
pub fn fail_text(err: EngineError) -> String {
    let code = err.code();
    format!(
        "Program error [{}]: Program error {code}: {err}",
        code.saturating_add(6000)
    )
}

/// Aborts the message, and with it the whole transaction, with the v0.2.4 error text.
pub fn fail(err: EngineError) -> ! {
    panic!("{}", fail_text(err))
}

/// The message must come from exactly `expected` (account and program).
pub fn expect_from(input: &ReceiveInput, expected: Actor, err: EngineError) {
    if input.from != Some(expected) {
        fail(err);
    }
}

/// The message must be submitted by the transaction itself, not sent by an actor.
pub fn expect_root(input: &ReceiveInput) {
    if input.from.is_some() {
        fail(EngineError::WrongAccount("root message expected"));
    }
}

/// The auction must be in `phase`.
pub fn expect_phase(state: &AuctionState, phase: AuctionPhase, err: EngineError) {
    if state.phase != phase {
        fail(err);
    }
}

/// The only sender `Seized` is accepted from (ruling R-5d): the requested position's actor at the
/// host program bound when the auction opened.
#[must_use]
pub fn expected_seized_sender(auction: &AuctionState) -> Actor {
    Actor::new(auction.position, auction.host.program_account_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{
        AuctionPhase,
        fixtures::{actor, auction, id},
    };
    use lee_core::program::ReceiveInput;

    fn input(from: Option<Actor>) -> ReceiveInput {
        ReceiveInput {
            receiver: actor(1, 2),
            from,
            is_authorized: false,
            pre_state: Vec::new().into(),
            message: Vec::new(),
        }
    }

    #[test]
    fn fail_text_matches_v024_format() {
        // v0.2.4: guest prefixes `[6000+code]` to SpelError::Custom's `Program error {code}: {Name}`.
        assert_eq!(
            fail_text(EngineError::NotAdmin),
            "Program error [6001]: Program error 1: NotAdmin"
        );
        assert_eq!(
            fail_text(EngineError::Math),
            "Program error [6099]: Program error 99: Math"
        );
        assert_eq!(
            fail_text(EngineError::WrongAccount("x")),
            "Program error [6012]: Program error 12: WrongAccount(\"x\")"
        );
    }

    #[test]
    #[should_panic(expected = "Program error [6034]: Program error 34: NotAuctioning")]
    fn fail_panics_with_the_text() {
        fail(EngineError::NotAuctioning)
    }

    #[test]
    fn expect_from_accepts_only_the_whole_actor() {
        let expected = actor(5, 6);
        expect_from(&input(Some(expected)), expected, EngineError::NotOwner);
        for from in [None, Some(actor(5, 7)), Some(actor(4, 6))] {
            let result = std::panic::catch_unwind(|| {
                expect_from(&input(from), expected, EngineError::NotOwner);
            });
            assert!(result.is_err(), "{from:?} must be refused");
        }
    }

    #[test]
    fn expect_root_refuses_a_sender() {
        expect_root(&input(None));
        assert!(std::panic::catch_unwind(|| expect_root(&input(Some(actor(1, 2))))).is_err());
    }

    #[test]
    fn expect_phase_compares_the_phase() {
        let state = auction(AuctionPhase::Auctioning);
        expect_phase(&state, AuctionPhase::Auctioning, EngineError::NotAuctioning);
        assert!(
            std::panic::catch_unwind(|| expect_phase(
                &state,
                AuctionPhase::Settled,
                EngineError::NotAuctioning
            ))
            .is_err()
        );
    }

    #[test]
    fn seized_is_accepted_only_from_the_requested_position_at_the_bound_host() {
        let mut state = auction(AuctionPhase::AwaitingSeize);
        state.position = id(30);
        state.host = actor(40, 41);
        assert_eq!(expected_seized_sender(&state), actor(30, 41));
        assert_ne!(
            expected_seized_sender(&state),
            state.host,
            "not the params actor"
        );
    }
}
