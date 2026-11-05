//! The config's leaf assertions of `SetEngineParams` (design note §4.2 row 1; v0.2.4
//! `set_engine_params`), as pure checks over the delivery so the sender branches are unit-tested:
//!
//! - `AssertCapAtMost` at a collateral actor: the type cap must not exceed the new global cap.
//! - `AssertSystemIdle` at an accounting actor: no surplus or debt auction may be live.
//!
//! Each accepts only its own config: the sender must be an actor of the receiver's program whose
//! account re-derives the receiver (ADR-057). Neither writes nor sends anything.
//!
//! Also the config's sender checks of the debt auction's `Mint` (row 14) and the accounting's
//! `Freeze` (row 16), whose account halves (an engine actor that is not this config's debt auction
//! or accounting) no transaction can reach.
//!
//! Likewise the accounting's sender checks of `System` (rows 9-16: the receiver/ctx and pending
//! halves) and of the surplus and debt auctions' reports (the sender re-derived from the reported
//! nonce), which no transaction can reach past their program half (Task 7 review, C1).

use auction_engine_params::EngineError;
use lee_core::{
    account::{AccountId, Actor},
    program::ReceiveInput,
};

use crate::{
    actors::{accounting_actor, collateral_actor, debt_actor, surplus_actor},
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

/// The collateral state at the receiver, whose config is exactly the sender.
fn collateral_of_sender(input: &ReceiveInput) -> Result<CollateralState, EngineError> {
    let config = engine_config_sender(input)?;
    let coll = CollateralState::decode(input.pre_state.as_ref())
        .ok_or(EngineError::WrongAccount("coll_config"))?;
    let engine = input.receiver.program_account_id;
    if coll.config != config
        || input.receiver != collateral_actor(engine, config, coll.collateral_definition_id)
    {
        return Err(EngineError::NotAdmin);
    }
    Ok(coll)
}

/// `SlackUpdate` at a collateral actor, from exactly its config: the decoded state to update.
pub fn slack_update(input: &ReceiveInput) -> Result<CollateralState, EngineError> {
    collateral_of_sender(input)
}

/// `AssertCapAtMost { global_cap }` at a collateral actor, from exactly its config.
pub fn cap_at_most(input: &ReceiveInput, global_cap: u128) -> Result<(), EngineError> {
    let coll = collateral_of_sender(input)?;
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

/// `Freeze` at a config `K` (row 16): the receiver holds a config state, returned
/// (`WrongAccount("config")`), and the sender is exactly `accounting_actor(E, K)`, the accounting
/// this config derives (`WrongAccount("accounting")`, ADR-057).
pub fn freeze_sender(input: &ReceiveInput) -> Result<ConfigState, EngineError> {
    let config =
        ConfigState::decode(input.pre_state.as_ref()).ok_or(EngineError::WrongAccount("config"))?;
    let k = input.receiver;
    if input.from != Some(accounting_actor(k.program_account_id, k.account_id)) {
        return Err(EngineError::WrongAccount("accounting"));
    }
    Ok(config)
}

/// `System` at an accounting actor (rows 9-16): the sender must be an actor `(k, E)` of the
/// receiver's program, the receiver exactly `accounting_actor(E, k)` and the ctx's config `k`
/// (`WrongAccount("accounting")`); the receiver holds an accounting state, returned
/// (`WrongAccount("accounting")`), with no system scratch pending (`WrongAccount("pending")`).
pub fn system_sender(
    input: &ReceiveInput,
    ctx_config: AccountId,
) -> Result<AccountingState, EngineError> {
    let wrong = EngineError::WrongAccount("accounting");
    let e = input.receiver.program_account_id;
    let k = input
        .from
        .filter(|from| from.program_account_id == e)
        .map(|from| from.account_id)
        .ok_or(wrong)?;
    if input.receiver != accounting_actor(e, k) || ctx_config != k {
        return Err(wrong);
    }
    let acc = AccountingState::decode(input.pre_state.as_ref()).ok_or(wrong)?;
    if acc.pending.is_some() {
        return Err(EngineError::WrongAccount("pending"));
    }
    Ok(acc)
}

/// A report at the accounting of `config` from exactly `expected` (`sender_err`), then the
/// receiver must be `accounting_actor(E, config)` holding an accounting state, returned
/// (`WrongAccount("accounting")`).
fn report_sender(
    input: &ReceiveInput,
    config: AccountId,
    expected: Actor,
    sender_err: EngineError,
) -> Result<AccountingState, EngineError> {
    if input.from != Some(expected) {
        return Err(sender_err);
    }
    let wrong = EngineError::WrongAccount("accounting");
    if input.receiver != accounting_actor(input.receiver.program_account_id, config) {
        return Err(wrong);
    }
    AccountingState::decode(input.pre_state.as_ref()).ok_or(wrong)
}

/// `SurplusTaken` / `SurplusReturned { config, nonce }` at the accounting: the sender is exactly
/// `surplus_actor(E, config, nonce)`, re-derived from the reported nonce
/// (`WrongAccount("surplus_auction")`); then as [`report_sender`].
pub fn surplus_report_sender(
    input: &ReceiveInput,
    config: AccountId,
    nonce: u64,
) -> Result<AccountingState, EngineError> {
    let s = surplus_actor(input.receiver.program_account_id, config, nonce);
    report_sender(
        input,
        config,
        s,
        EngineError::WrongAccount("surplus_auction"),
    )
}

/// `DebtTaken` / `DebtClosed { config, nonce }` at the accounting: the sender is exactly
/// `debt_actor(E, config, nonce)`, re-derived from the reported nonce
/// (`WrongAccount("debt_auction")`); then as [`report_sender`].
pub fn debt_report_sender(
    input: &ReceiveInput,
    config: AccountId,
    nonce: u64,
) -> Result<AccountingState, EngineError> {
    let d = debt_actor(input.receiver.program_account_id, config, nonce);
    report_sender(input, config, d, EngineError::WrongAccount("debt_auction"))
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

    /// `Freeze` at `K`: only `accounting_actor(E, K)`; not another config's accounting, not an
    /// impostor program at the accounting's account, not `K` itself, not a root delivery.
    #[test]
    fn freeze_is_accepted_only_from_this_configs_accounting() {
        let (admin, k, k2) = actors();
        let config = crate::state::fixtures::config();
        let acc = accounting_actor(engine(), k.account_id);
        let others = [
            None,
            Some(admin),
            Some(k),
            Some(k2),
            Some(accounting_actor(engine(), k2.account_id)),
            Some(Actor::new(acc.account_id, id(77))),
        ];
        for from in others {
            assert_eq!(
                freeze_sender(&delivery(k, from, config.encode())),
                Err(EngineError::WrongAccount("accounting")),
                "{from:?}"
            );
        }
        assert_eq!(
            freeze_sender(&delivery(k, Some(acc), config.encode())),
            Ok(config)
        );
        assert_eq!(
            freeze_sender(&delivery(k, Some(acc), Vec::new())),
            Err(EngineError::WrongAccount("config"))
        );
    }

    /// `System` at `Acc = accounting_actor(E, K)`: only from `K` at `E`, naming `K` in the ctx,
    /// at `K`'s own accounting, with an accounting state and no scratch; each check in order.
    #[test]
    fn system_is_accepted_only_from_the_accountings_own_config() {
        let (admin, k, k2) = actors();
        let kid = k.account_id;
        let acc = accounting_actor(engine(), kid);
        let idle = AccountingState::new();
        let at = |receiver, from, state: &AccountingState| delivery(receiver, from, state.encode());
        let wrong = Err(EngineError::WrongAccount("accounting"));
        // Program half: a root delivery, or a sender not at E (even at K's account).
        for from in [None, Some(Actor::new(kid, id(77))), Some(admin)] {
            assert_eq!(system_sender(&at(acc, from, &idle), kid), wrong, "{from:?}");
        }
        // Receiver half: `K` at E sending to another config's accounting (or to itself).
        for receiver in [accounting_actor(engine(), k2.account_id), k] {
            assert_eq!(
                system_sender(&at(receiver, Some(k), &idle), kid),
                wrong,
                "{receiver:?}"
            );
        }
        // Another config K2 at E, at K's accounting.
        assert_eq!(system_sender(&at(acc, Some(k2), &idle), kid), wrong);
        // Ctx half: the right sender and receiver, a ctx naming another config.
        assert_eq!(
            system_sender(&at(acc, Some(k), &idle), k2.account_id),
            wrong
        );
        // No accounting state.
        assert_eq!(
            system_sender(&delivery(acc, Some(k), Vec::new()), kid),
            wrong
        );
        // A scratch still pending.
        let busy = AccountingState {
            pending: Some(crate::SystemPending {
                op: crate::SystemOp::SettleDebt { amount: 1 },
                now_ms: 0,
                ctx: crate::SystemCtx::from_config(kid, &crate::state::fixtures::config()),
                buffer: None,
                reference: None,
            }),
            ..AccountingState::new()
        };
        assert_eq!(
            system_sender(&at(acc, Some(k), &busy), kid),
            Err(EngineError::WrongAccount("pending"))
        );
        assert_eq!(system_sender(&at(acc, Some(k), &idle), kid), Ok(idle));
    }

    /// Shared body of the two report-sender tests: `check` accepts a report for nonce 3 at `K`'s
    /// accounting only from `own(E, K, 3)` (`sender_err` otherwise: a root delivery, `K`, another
    /// nonce's auction, another config's, an impostor program at `own`'s account, or the right
    /// auction reporting another nonce), then needs that receiver and an accounting state.
    fn report_sender_cases(
        own: fn(AccountId, AccountId, u64) -> Actor,
        check: fn(&ReceiveInput, AccountId, u64) -> Result<AccountingState, EngineError>,
        sender_err: EngineError,
    ) {
        let (_, k, k2) = actors();
        let (e, kid) = (engine(), k.account_id);
        let acc = accounting_actor(e, kid);
        let reporter = own(e, kid, 3);
        let books = AccountingState::new();
        let foreign = [
            None,
            Some(k),
            Some(own(e, kid, 4)),
            Some(own(e, kid, 2)),
            Some(own(e, k2.account_id, 3)),
            Some(Actor::new(reporter.account_id, id(77))),
        ];
        for from in foreign {
            assert_eq!(
                check(&delivery(acc, from, books.encode()), kid, 3),
                Err(sender_err),
                "{from:?}"
            );
        }
        // The real auction 3, reporting nonce 4: the sender is re-derived from the nonce.
        assert_eq!(
            check(&delivery(acc, Some(reporter), books.encode()), kid, 4),
            Err(sender_err)
        );
        // The right reporter, at another config's accounting (or at K itself).
        for receiver in [accounting_actor(e, k2.account_id), k] {
            assert_eq!(
                check(&delivery(receiver, Some(reporter), books.encode()), kid, 3),
                Err(EngineError::WrongAccount("accounting")),
                "{receiver:?}"
            );
        }
        assert_eq!(
            check(&delivery(acc, Some(reporter), Vec::new()), kid, 3),
            Err(EngineError::WrongAccount("accounting"))
        );
        assert_eq!(
            check(&delivery(acc, Some(reporter), books.encode()), kid, 3),
            Ok(books)
        );
    }

    /// `SurplusTaken` / `SurplusReturned` for nonce `n`: only from `surplus_actor(E, K, n)`.
    #[test]
    fn surplus_reports_are_accepted_only_from_the_reported_nonces_auction() {
        report_sender_cases(
            surplus_actor,
            surplus_report_sender,
            EngineError::WrongAccount("surplus_auction"),
        );
    }

    /// `DebtTaken` / `DebtClosed` for nonce `n`: only from `debt_actor(E, K, n)`.
    #[test]
    fn debt_reports_are_accepted_only_from_the_reported_nonces_auction() {
        report_sender_cases(
            debt_actor,
            debt_report_sender,
            EngineError::WrongAccount("debt_auction"),
        );
    }
}
