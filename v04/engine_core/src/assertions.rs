//! The config's leaf assertions of `SetEngineParams` (design note §4.2 row 1; v0.2.4
//! `set_engine_params`), as pure checks over the delivery so the sender branches are unit-tested:
//!
//! - `AssertCapAtMost` at a collateral actor: the type cap must not exceed the new global cap.
//! - `AssertSystemIdle` at an accounting actor: no surplus or debt auction may be live.
//!
//! Each accepts only its own config: the sender must be an actor of the receiver's program whose
//! account re-derives the receiver (ADR-057). Neither writes nor sends anything.
//!
//! Also the config's sender check of the debt auction's `Mint` (row 14), whose account half (an
//! engine actor that is not this config's debt auction) no transaction can reach.

use auction_engine_params::EngineError;
use lee_core::{
    account::{AccountId, Actor},
    program::ReceiveInput,
};

use crate::{
    actors::{accounting_actor, collateral_actor, debt_actor},
    state::{AccountingState, CollateralState, ConfigState},
};

/// The sending config's account: the sender must be an actor of the receiver's program, else
/// `NotAdmin`.
fn engine_config_sender(input: &ReceiveInput) -> Result<AccountId, EngineError> {
    let engine = input.receiver.program_account_id;
    match input.from {
        Some(from) if from == Actor::new(from.account_id, engine) => Ok(from.account_id),
        _ => Err(EngineError::NotAdmin),
    }
}

/// `AssertCapAtMost { global_cap }` at a collateral actor, from exactly its config.
pub fn cap_at_most(input: &ReceiveInput, global_cap: u128) -> Result<(), EngineError> {
    let config = engine_config_sender(input)?;
    let coll = CollateralState::decode(input.pre_state.as_ref())
        .ok_or(EngineError::WrongAccount("coll_config"))?;
    let engine = input.receiver.program_account_id;
    if coll.config != config
        || input.receiver != collateral_actor(engine, config, coll.collateral_definition_id)
    {
        return Err(EngineError::NotAdmin);
    }
    if coll.type_cap > global_cap {
        return Err(EngineError::TypeCapAboveGlobal);
    }
    Ok(())
}

/// `AssertSystemIdle` at an accounting actor, from exactly its config.
pub fn system_idle(input: &ReceiveInput) -> Result<(), EngineError> {
    let config = engine_config_sender(input)?;
    if input.receiver != accounting_actor(input.receiver.program_account_id, config) {
        return Err(EngineError::NotAdmin);
    }
    let accounting = AccountingState::decode(input.pre_state.as_ref())
        .ok_or(EngineError::WrongAccount("accounting"))?;
    if accounting.surplus_live || accounting.debt_live {
        return Err(EngineError::ProtocolTokenLocked);
    }
    Ok(())
}

/// `Mint { nonce, .. }` at a config `K` (row 14): the receiver holds a config state
/// (`WrongAccount("config")`) and the sender is exactly `debt_actor(E, K, nonce)`, the auction
/// this config derives for that nonce (`WrongAccount("debt_auction")`, ADR-057).
pub fn mint_sender(input: &ReceiveInput, nonce: u64) -> Result<(), EngineError> {
    if ConfigState::decode(input.pre_state.as_ref()).is_none() {
        return Err(EngineError::WrongAccount("config"));
    }
    let k = input.receiver;
    if input.from != Some(debt_actor(k.program_account_id, k.account_id, nonce)) {
        return Err(EngineError::WrongAccount("debt_auction"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        actors::config_actor,
        state::fixtures::{coll, id},
    };

    const ENGINE: u8 = 9;

    fn engine() -> AccountId {
        id(ENGINE)
    }

    /// The admin, its config `K`, and another admin's config `K2`.
    fn actors() -> (Actor, Actor, Actor) {
        let admin = Actor::new(id(1), engine());
        (
            admin,
            config_actor(engine(), id(1), 1),
            config_actor(engine(), id(2), 1),
        )
    }

    fn delivery(receiver: Actor, from: Option<Actor>, state: Vec<u8>) -> ReceiveInput {
        ReceiveInput {
            receiver,
            from,
            is_authorized: false,
            pre_state: state.into(),
            message: Vec::new(),
        }
    }

    /// `K`'s collateral actor for definition 4, cap 5.
    fn collateral_at(k: Actor) -> (Actor, Vec<u8>) {
        let state = CollateralState {
            config: k.account_id,
            ..coll()
        };
        let at = collateral_actor(engine(), k.account_id, state.collateral_definition_id);
        (at, state.encode())
    }

    #[test]
    fn cap_assertion_accepts_only_its_config() {
        let (admin, k, k2) = actors();
        let (c, state) = collateral_at(k);
        let impostor = Actor::new(k.account_id, id(77));
        for from in [None, Some(admin), Some(k2), Some(impostor)] {
            assert_eq!(
                cap_at_most(&delivery(c, from, state.clone()), 5),
                Err(EngineError::NotAdmin),
                "{from:?}"
            );
        }
        assert_eq!(cap_at_most(&delivery(c, Some(k), state.clone()), 5), Ok(()));
        assert_eq!(
            cap_at_most(&delivery(c, Some(k), state), 4),
            Err(EngineError::TypeCapAboveGlobal)
        );
        assert_eq!(
            cap_at_most(&delivery(c, Some(k), Vec::new()), 5),
            Err(EngineError::WrongAccount("coll_config"))
        );
    }

    #[test]
    fn idle_assertion_accepts_only_its_config() {
        let (admin, k, k2) = actors();
        let acc = accounting_actor(engine(), k.account_id);
        let idle = AccountingState::new().encode();
        let impostor = Actor::new(k.account_id, id(77));
        for from in [None, Some(admin), Some(k2), Some(impostor)] {
            assert_eq!(
                system_idle(&delivery(acc, from, idle.clone())),
                Err(EngineError::NotAdmin),
                "{from:?}"
            );
        }
        assert_eq!(system_idle(&delivery(acc, Some(k), idle)), Ok(()));
        for (surplus_live, debt_live) in [(true, false), (false, true)] {
            let live = AccountingState {
                surplus_live,
                debt_live,
                ..AccountingState::new()
            };
            assert_eq!(
                system_idle(&delivery(acc, Some(k), live.encode())),
                Err(EngineError::ProtocolTokenLocked)
            );
        }
        assert_eq!(
            system_idle(&delivery(acc, Some(k), Vec::new())),
            Err(EngineError::WrongAccount("accounting"))
        );
    }

    /// `Mint` at `K`: only `debt_actor(E, K, nonce)`; not another nonce's auction, not another
    /// config's `D'`, not an impostor program at `D`'s account, not a root delivery.
    #[test]
    fn mint_is_accepted_only_from_this_configs_debt_actor() {
        let (admin, k, k2) = actors();
        let config = crate::state::fixtures::config().encode();
        let d = debt_actor(engine(), k.account_id, 3);
        let impostor = Actor::new(d.account_id, id(77));
        let others = [
            None,
            Some(admin),
            Some(k2),
            Some(debt_actor(engine(), k.account_id, 4)),
            Some(debt_actor(engine(), k2.account_id, 3)),
            Some(impostor),
        ];
        for from in others {
            assert_eq!(
                mint_sender(&delivery(k, from, config.clone()), 3),
                Err(EngineError::WrongAccount("debt_auction")),
                "{from:?}"
            );
        }
        assert_eq!(mint_sender(&delivery(k, Some(d), config), 3), Ok(()));
        assert_eq!(
            mint_sender(&delivery(k, Some(d), Vec::new()), 3),
            Err(EngineError::WrongAccount("config"))
        );
    }
}
