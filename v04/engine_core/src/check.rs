//! Checks every handler starts with (design note §3: compare whole actors, never bare ids).

use auction_engine_params::EngineError;
use lee_core::{
    account::{AccountId, Actor},
    program::ReceiveInput,
};

use crate::state::{AuctionPhase, AuctionState, ConfigState};

/// The least clock slack the config accepts, ms (ADR-064): below it a timed transaction can
/// miss the claim window at the sequencer's block spacing and revert at its full gas.
pub const MIN_SLACK_MS: u64 = 1_000;
/// The most clock slack the config accepts, ms (ADR-064): it bounds how far a claim can be
/// backdated.
pub const MAX_SLACK_MS: u64 = 300_000;

/// `slack_ms` must lie in `MIN_SLACK_MS..=MAX_SLACK_MS`.
///
/// # Errors
/// [`EngineError::InvalidConfig`] outside the range.
pub const fn slack_in_range(slack_ms: u64) -> Result<(), EngineError> {
    if slack_ms < MIN_SLACK_MS || slack_ms > MAX_SLACK_MS {
        return Err(EngineError::InvalidConfig);
    }
    Ok(())
}

/// What the host's parameters must say about the config that names it (ADR-065): its stablecoin
/// definition and token program are the config's, and its oracle is the config's oracle. This
/// reads the mock host's `Params` layout, so it is re-pointed when the upstream host lands.
///
/// # Errors
/// [`EngineError::HostMismatch`] if `stable_def` or `token` differ;
/// [`EngineError::OracleMismatch`] if `oracle` differs.
pub fn host_params_match(
    config: &ConfigState,
    stable_def: AccountId,
    token: AccountId,
    oracle: Actor,
) -> Result<(), EngineError> {
    if stable_def != config.stablecoin_definition_id || token != config.token {
        return Err(EngineError::HostMismatch);
    }
    if oracle != config.oracle {
        return Err(EngineError::OracleMismatch);
    }
    Ok(())
}

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

    fn matches_config(stable_def: AccountId, token: AccountId, oracle: Actor) -> Option<u32> {
        let config = crate::state::fixtures::config();
        host_params_match(&config, stable_def, token, oracle)
            .err()
            .map(|err| err.code())
    }

    #[test]
    fn host_params_match_accepts_equal() {
        let c = crate::state::fixtures::config();
        assert_eq!(
            matches_config(c.stablecoin_definition_id, c.token, c.oracle),
            None
        );
    }

    #[test]
    fn host_params_match_rejects_stable_def() {
        let c = crate::state::fixtures::config();
        assert_eq!(matches_config(id(99), c.token, c.oracle), Some(11));
    }

    #[test]
    fn host_params_match_rejects_token() {
        let c = crate::state::fixtures::config();
        assert_eq!(
            matches_config(c.stablecoin_definition_id, id(99), c.oracle),
            Some(11)
        );
    }

    #[test]
    fn host_params_match_rejects_oracle() {
        let c = crate::state::fixtures::config();
        assert_eq!(
            matches_config(c.stablecoin_definition_id, c.token, actor(99, 98)),
            Some(13)
        );
        // The whole actor: the right account at another program is another oracle.
        let other_program = Actor::new(c.oracle.account_id, id(97));
        assert_eq!(
            matches_config(c.stablecoin_definition_id, c.token, other_program),
            Some(13)
        );
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
