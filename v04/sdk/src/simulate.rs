//! The pure simulation core (ruling R-P3): a plan simulated against any [`StateSource`] — the
//! live sequencer (through [`crate::live::Snapshot`]) or the in-process harness — and gas fitting.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use lee::{
    AccountId, Actor, FeeDeclaration, PublicAccountEvidence, PublicTransaction, Simulation,
    privacy_preserving_transaction::circuit::ProgramCatalog,
    program::DEFAULT_PUBLIC_CYCLE_BUDGET,
    public_transaction::{Message, WitnessSet},
};
use lee_core::account::{ActorState, Nonce};

use crate::{Plan, SdkError};

/// The per-block execution gas cap no transaction may declare more than (`max_gas_exec`, S8).
pub const MAX_GAS_EXEC: u64 = 10_000_000;

/// The state simulation reads: actor states and which accounts exist.
pub trait StateSource {
    /// The raw state `actor.account_id` holds for `actor.program_account_id`, `None` if absent.
    fn actor_state(&self, actor: Actor) -> Option<Vec<u8>>;

    /// Whether the account exists, the condition settlement admits an unsigned account on.
    fn account_exists(&self, account: AccountId) -> bool;
}

/// `ceil(simulated_cycles × 1.2)`, the gas limit a send declares.
///
/// # Errors
/// [`SdkError::GasAboveCap`] above [`MAX_GAS_EXEC`].
pub fn fit_gas(simulated_cycles: u64) -> Result<u64, SdkError> {
    let gas = u128::from(simulated_cycles).saturating_mul(6).div_ceil(5);
    match u64::try_from(gas) {
        Ok(gas) if gas <= MAX_GAS_EXEC => Ok(gas),
        Ok(gas) => Err(SdkError::GasAboveCap {
            gas,
            cap: MAX_GAS_EXEC,
        }),
        Err(_) => Err(SdkError::GasAboveCap {
            gas: u64::MAX,
            cap: MAX_GAS_EXEC,
        }),
    }
}

/// Simulates `plan` paid by `payer` against `source` with `programs`, admitting what settlement
/// would: the signers and the payer, every declared account that exists, and every account the
/// plan's evidence proves public. Returns the cycles used.
///
/// The cycle budget is LEZ's public default (32M), above [`MAX_GAS_EXEC`], so a plan that runs
/// but needs more gas than the cap is reported by [`fit_gas`], not as a revert.
///
/// # Errors
/// [`SdkError::WouldRevert`] with LEZ's reason if the transaction would fail.
pub fn simulate_with(
    source: &impl StateSource,
    programs: &ProgramCatalog,
    plan: &Plan,
    payer: AccountId,
) -> Result<u64, SdkError> {
    let nonces = plan
        .signers
        .iter()
        .chain([&payer])
        .map(|account| (*account, Nonce(0)))
        .collect();
    let tx = unsigned_transaction(plan, nonces, None);
    let mut admitted: BTreeSet<AccountId> = plan.signers.iter().copied().collect();
    admitted.insert(payer);
    admitted.extend(plan.evidence.iter().map(PublicAccountEvidence::account_id));
    let mut states = HashMap::new();
    for actor in &plan.actors {
        if source.account_exists(actor.account_id) {
            admitted.insert(actor.account_id);
        }
        if let Some(state) = source.actor_state(*actor) {
            states.insert(*actor, ActorState::from(state));
        }
    }
    let simulation = Simulation {
        public_actor_states: states,
        admitted_accounts: Some(admitted),
    };
    let (cycles, result) =
        lee::simulate_public_transaction(&tx, &simulation, programs, DEFAULT_PUBLIC_CYCLE_BUDGET);
    result.map_err(|err| SdkError::WouldRevert(format!("{err}: {err:?}")))?;
    Ok(cycles)
}

/// `plan` as a public transaction message with `nonces` and `fee`, carrying its evidence.
pub(crate) fn message(
    plan: &Plan,
    nonces: BTreeMap<AccountId, Nonce>,
    fee: Option<FeeDeclaration>,
) -> Message {
    Message {
        admission_evidence: plan.evidence.clone(),
        ..Message::new_preserialized(
            plan.root,
            plan.message.clone(),
            plan.actors.iter().copied(),
            nonces,
            fee,
        )
    }
}

/// `plan` as a transaction without signatures (simulation does not verify witnesses).
fn unsigned_transaction(
    plan: &Plan,
    nonces: BTreeMap<AccountId, Nonce>,
    fee: Option<FeeDeclaration>,
) -> PublicTransaction {
    let message = message(plan, nonces, fee);
    let witness = WitnessSet::for_message(&message, &[]);
    PublicTransaction::new(message, witness)
}
