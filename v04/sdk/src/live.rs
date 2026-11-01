//! Simulating and sending plans over a live sequencer: the async wrappers around
//! [`simulate_with`] (ruling R-P3), with the sequencer read into a [`Snapshot`].
//!
//! Fee declaration of a send: `gas_limit = fit_gas(simulated cycles)`, `tip = 0`, and
//! `max_fee = gas_limit × exec price + (wire bytes + STORAGE_MARGIN_BYTES) × storage price`
//! (see [`max_fee`]), where the wire bytes are the borsh size of the signed `LeeTransaction` —
//! what settlement bills as storage gas — and each price is the higher of the head base fee and
//! the next block's ceiling from `getFeeState`, so a one-block fee step between quote and
//! inclusion cannot push the fee reserve above `max_fee`. The payer is billed the actual fee
//! (cycles used, not the limit), never `max_fee`.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use common::{HashType, transaction::LeeTransaction};
use lee::{
    Account, AccountId, FeeDeclaration, PrivateKey, PublicKey, PublicTransaction,
    privacy_preserving_transaction::circuit::{Dependency, ProgramCatalog, ProgramKind},
    public_transaction::WitnessSet,
};
use lee_core::BlockId;
use sequencer_service_rpc::{FeeStateQuote, RpcClient as _, SequencerClient};
use wallet::{WalletCore, program_facades::program_loader::ProgramLoader};

use crate::{
    Plan, SdkError,
    simulate::{StateSource, fit_gas, message, simulate_with},
};

/// The wallet type the SDK signs and loads programs with.
pub type Wallet = WalletCore<SequencerClient>;

/// Bytes of storage gas `max_fee` allows beyond the measured wire size. The measured size is
/// exact (`max_fee` is a fixed-width u128, so pricing it does not change the size); this is a
/// cushion only.
pub const STORAGE_MARGIN_BYTES: u64 = 64;

/// Accounts read from the sequencer at one moment; a [`StateSource`] over exactly those accounts
/// (an account not fetched reads as absent).
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    accounts: BTreeMap<AccountId, Option<Account>>,
}

impl Snapshot {
    /// Reads `accounts` with `getAccount`.
    ///
    /// # Errors
    /// [`SdkError::Rpc`] if the sequencer fails.
    pub async fn fetch(
        client: &SequencerClient,
        accounts: impl IntoIterator<Item = AccountId>,
    ) -> Result<Self, SdkError> {
        let mut out = BTreeMap::new();
        for account in accounts.into_iter().collect::<BTreeSet<_>>() {
            let fetched = client
                .get_account(account)
                .await
                .map_err(|err| SdkError::Rpc(format!("getAccount {account}: {err}")))?;
            out.insert(account, fetched);
        }
        Ok(Self { accounts: out })
    }

    /// Reads every account `plan` declares, its signers and `payer`.
    ///
    /// # Errors
    /// [`SdkError::Rpc`] if the sequencer fails.
    pub async fn of_plan(
        client: &SequencerClient,
        plan: &Plan,
        payer: AccountId,
    ) -> Result<Self, SdkError> {
        let accounts = plan
            .actors
            .iter()
            .map(|actor| actor.account_id)
            .chain(plan.signers.iter().copied())
            .chain([payer]);
        Self::fetch(client, accounts).await
    }
}

impl StateSource for Snapshot {
    fn actor_state(&self, actor: lee::Actor) -> Option<Vec<u8>> {
        self.accounts
            .get(&actor.account_id)?
            .as_ref()?
            .data
            .actor_states
            .get(&actor.program_account_id)
            .map(|state| state.to_vec())
    }

    fn account_exists(&self, account: AccountId) -> bool {
        matches!(self.accounts.get(&account), Some(Some(_)))
    }
}

/// The deployed programs of every actor `plan` declares, loaded through the program loader.
///
/// A program the loader does not find is left out rather than reported: an actor whose program
/// is only observed, never invoked (e.g. the clock system program's clock account), needs no
/// code, and an invoked program that is missing makes [`simulate_with`] return
/// [`SdkError::WouldRevert`] naming it.
///
/// # Errors
/// [`SdkError::Rpc`] if the sequencer fails.
pub async fn load_programs(wallet: &Wallet, plan: &Plan) -> Result<ProgramCatalog, SdkError> {
    let mut programs = ProgramCatalog::default();
    let wanted: BTreeSet<AccountId> = plan
        .actors
        .iter()
        .map(|actor| actor.program_account_id)
        .collect();
    for id in wanted {
        let loaded = ProgramLoader(wallet)
            .program(id)
            .await
            .map_err(|err| SdkError::Rpc(format!("loading program {id}: {err}")))?;
        if let Some(program) = loaded {
            programs.programs.insert(
                id,
                Dependency {
                    program,
                    kind: ProgramKind::Disclosed,
                },
            );
        }
    }
    Ok(programs)
}

/// Simulates `plan` paid by `payer` against the sequencer's current state: live actor states of
/// every declared actor, programs via the loader, admitted set = signers ∪ existing accounts ∪
/// evidence. Returns the cycles used.
///
/// # Errors
/// [`SdkError::WouldRevert`] if it would fail; [`SdkError::Rpc`] if the sequencer fails.
pub async fn simulate(
    client: &SequencerClient,
    wallet: &Wallet,
    plan: &Plan,
    payer: AccountId,
) -> Result<u64, SdkError> {
    let snapshot = Snapshot::of_plan(client, plan, payer).await?;
    let programs = load_programs(wallet, plan).await?;
    simulate_with(&snapshot, &programs, plan, payer)
}

/// `gas_limit × exec price + (wire_bytes + STORAGE_MARGIN_BYTES) × storage price`, each price
/// the higher of the quote's head base fee and next-block ceiling.
#[must_use]
pub fn max_fee(gas_limit: u64, wire_bytes: u64, quote: &FeeStateQuote) -> u128 {
    let exec = quote.base_fee_exec.max(quote.next_base_fee_exec_ceiling);
    let stor = quote.base_fee_stor.max(quote.next_base_fee_stor_ceiling);
    let storage = u128::from(wire_bytes.saturating_add(STORAGE_MARGIN_BYTES));
    u128::from(gas_limit)
        .saturating_mul(u128::from(exec))
        .saturating_add(storage.saturating_mul(u128::from(stor)))
}

/// What [`send`] declared for a transaction it handed to the sequencer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sent {
    /// The transaction hash.
    pub hash: [u8; 32],
    /// The cycles the simulation used.
    pub cycles: u64,
    /// `fit_gas(cycles)`, the declared gas limit.
    pub gas_limit: u64,
    /// The declared fee ceiling (see [`max_fee`]).
    pub max_fee: u128,
}

/// Simulates `plan`, fits its gas, declares the fee (see the module docs), signs it with the
/// payer's and the plan signers' wallet keys and sends it.
///
/// # Errors
/// [`SdkError::MissingKey`] if the wallet cannot sign; otherwise as [`send_signed`].
pub async fn send(
    client: &SequencerClient,
    wallet: &Wallet,
    plan: &Plan,
    payer: AccountId,
) -> Result<Sent, SdkError> {
    let keys = signing_accounts(plan, payer)
        .into_iter()
        .map(|account| {
            wallet
                .storage()
                .key_chain()
                .pub_account_signing_key(account)
                .ok_or(SdkError::MissingKey(account))
        })
        .collect::<Result<Vec<&PrivateKey>, _>>()?;
    send_signed(client, wallet, plan, payer, &keys).await
}

/// [`send`] signed with `keys` instead of the wallet's: `keys` must hold the payer's key and
/// every plan signer's (extra keys are ignored). The wallet is used only to load programs.
///
/// # Errors
/// [`SdkError::WouldRevert`] / [`SdkError::GasAboveCap`] before anything is sent;
/// [`SdkError::MissingKey`] if `keys` lack a required signer; [`SdkError::Rpc`] if the sequencer
/// fails or refuses the transaction.
pub async fn send_signed(
    client: &SequencerClient,
    wallet: &Wallet,
    plan: &Plan,
    payer: AccountId,
    keys: &[&PrivateKey],
) -> Result<Sent, SdkError> {
    let accounts = signing_accounts(plan, payer);
    let by_account: BTreeMap<AccountId, &PrivateKey> = keys
        .iter()
        .map(|key| (AccountId::from(&PublicKey::new_from_private_key(key)), *key))
        .collect();
    let keys = accounts
        .iter()
        .map(|account| {
            by_account
                .get(account)
                .copied()
                .ok_or(SdkError::MissingKey(*account))
        })
        .collect::<Result<Vec<&PrivateKey>, _>>()?;
    let cycles = simulate(client, wallet, plan, payer).await?;
    let gas_limit = fit_gas(cycles)?;
    let quote = client
        .get_fee_state()
        .await
        .map_err(|err| SdkError::Rpc(format!("getFeeState: {err}")))?;
    let nonces = client
        .get_accounts_nonces(accounts.clone())
        .await
        .map_err(|err| SdkError::Rpc(format!("getAccountsNonces: {err}")))?;
    let nonces: BTreeMap<_, _> = accounts.iter().copied().zip(nonces).collect();
    let fee = |max_fee| FeeDeclaration {
        payer,
        gas_limit,
        tip: 0,
        max_fee,
    };
    let sized = signed(plan, &nonces, fee(0), &keys);
    let wire_bytes = wire_size(&sized)?;
    let max_fee = max_fee(gas_limit, wire_bytes, &quote);
    let tx = signed(plan, &nonces, fee(max_fee), &keys);
    let hash = client
        .send_transaction(LeeTransaction::Public(tx))
        .await
        .map_err(|err| SdkError::Rpc(format!("sendTransaction: {err}")))?;
    Ok(Sent {
        hash: hash.0,
        cycles,
        gas_limit,
        max_fee,
    })
}

/// Waits until the transaction `hash` is in a block (polling `getTransaction` every `poll`, at
/// most `timeout`), then checks the block's `reverted` list. Returns the block id.
///
/// A reverted transaction is still included and charged (S8), so inclusion alone does not mean
/// it applied.
///
/// # Errors
/// [`SdkError::Reverted`] if the block marks it reverted; [`SdkError::Rpc`] if the sequencer
/// fails or the transaction is not included within `timeout`.
pub async fn confirm(
    client: &SequencerClient,
    hash: [u8; 32],
    poll: Duration,
    timeout: Duration,
) -> Result<BlockId, SdkError> {
    let started = Instant::now();
    let block_id = loop {
        let found = client
            .get_transaction(HashType(hash))
            .await
            .map_err(|err| SdkError::Rpc(format!("getTransaction: {err}")))?;
        if let Some((_, block_id)) = found {
            break block_id;
        }
        if started.elapsed() >= timeout {
            return Err(SdkError::Rpc(format!(
                "transaction {} not included within {timeout:?}",
                HashType(hash)
            )));
        }
        tokio::time::sleep(poll).await;
    };
    let block = client
        .get_block(block_id)
        .await
        .map_err(|err| SdkError::Rpc(format!("getBlock {block_id}: {err}")))?
        .ok_or_else(|| SdkError::Rpc(format!("block {block_id} not found")))?;
    let position = block
        .body
        .transactions
        .iter()
        .position(|tx| tx.hash().0 == hash)
        .ok_or_else(|| {
            SdkError::Rpc(format!(
                "block {block_id} does not list transaction {}",
                HashType(hash)
            ))
        })?;
    if block.body.is_reverted(position) {
        return Err(SdkError::Reverted(format!(
            "transaction {} reverted in block {block_id}",
            HashType(hash)
        )));
    }
    Ok(block_id)
}

/// The payer and the plan's signers, deduplicated and ordered: the accounts a send signs for.
fn signing_accounts(plan: &Plan, payer: AccountId) -> Vec<AccountId> {
    [payer]
        .into_iter()
        .chain(plan.signers.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn signed(
    plan: &Plan,
    nonces: &BTreeMap<AccountId, lee_core::account::Nonce>,
    fee: FeeDeclaration,
    keys: &[&PrivateKey],
) -> PublicTransaction {
    let message = message(plan, nonces.clone(), Some(fee));
    let witness = WitnessSet::for_message(&message, keys);
    PublicTransaction::new(message, witness)
}

/// The storage gas settlement bills: the borsh size of the whole `LeeTransaction`.
fn wire_size(tx: &PublicTransaction) -> Result<u64, SdkError> {
    let bytes = borsh::to_vec(&LeeTransaction::Public(tx.clone()))
        .map_err(|err| SdkError::Encode(err.to_string()))?;
    u64::try_from(bytes.len()).map_err(|err| SdkError::Encode(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(exec: u64, exec_ceiling: u64, stor: u64, stor_ceiling: u64) -> FeeStateQuote {
        FeeStateQuote {
            height: 1,
            base_fee_exec: exec,
            base_fee_stor: stor,
            next_base_fee_exec_floor: exec,
            next_base_fee_exec_ceiling: exec_ceiling,
            next_base_fee_stor_floor: stor,
            next_base_fee_stor_ceiling: stor_ceiling,
            max_gas_exec: crate::MAX_GAS_EXEC,
            max_gas_stor: 1_000_000,
        }
    }

    #[test]
    fn max_fee_prices_gas_and_wire_bytes_at_the_next_block_ceiling() {
        // 1_000 gas × 9 + (436 + 64) bytes × 10.
        assert_eq!(max_fee(1_000, 436, &quote(8, 9, 8, 10)), 9_000 + 5_000);
        // A falling market never prices below the head.
        assert_eq!(max_fee(1_000, 436, &quote(8, 7, 8, 7)), 8_000 + 4_000);
        assert_eq!(
            max_fee(u64::MAX, u64::MAX, &quote(u64::MAX, 0, u64::MAX, 0)),
            u128::MAX
        );
    }
}
