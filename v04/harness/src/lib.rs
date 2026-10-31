//! In-process test harness on LEZ v0.4.0-rc1: a `lee::V03State` with our guests and the clock
//! system program installed, signed public transactions, simulation, and harness-set time; and
//! [`World`], the deployed token, oracle, mock host and engine the engine tests share.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap},
};

use borsh::{BorshDeserialize, BorshSerialize};
use clock_core::{CLOCK_01_PROGRAM_ACCOUNT_ID, CLOCK_PROGRAM_ACCOUNT_IDS, ClockAccountData};
use lee::{
    Account, AccountId, Actor, PrivateKey, PublicAccountEvidence, PublicKey, PublicTransaction,
    Simulation, V03State,
    error::LeeError,
    privacy_preserving_transaction::circuit::ProgramCatalog,
    program::{DEFAULT_PUBLIC_CYCLE_BUDGET, Program},
    public_transaction::{Message, WitnessSet},
};
use lee_core::{BlockId, Timestamp, program::TransactionEvent};
use sha2::{Digest as _, Sha256};

pub mod world;
pub use world::World;

/// The name `Chain::program` knows the clock system program by.
pub const CLOCK: &str = "clock";

/// A single-node chain held in memory. Every `send` is its own block.
pub struct Chain {
    state: V03State,
    catalog: ProgramCatalog,
    programs: BTreeMap<&'static str, AccountId>,
    keys: BTreeMap<String, PrivateKey>,
    block: BlockId,
    now_ms: Timestamp,
}

impl Chain {
    /// Installs each `(name, elf)` at its builtin address and the clock program at its
    /// name-derived address with its three clock accounts, as LEZ's testnet genesis does.
    pub fn new(programs: &[(&'static str, &'static [u8])]) -> Self {
        let installed: Vec<(&'static str, AccountId, Program)> = programs
            .iter()
            .map(|&(name, elf)| {
                let program = Program::new(Cow::Borrowed(elf)).expect("a valid guest ELF");
                (name, AccountId::from_builtin_program(program.id()), program)
            })
            .chain([(
                CLOCK,
                lez_programs::clock_account_id(),
                lez_programs::clock(),
            )])
            .collect();
        let names: BTreeMap<&'static str, AccountId> =
            installed.iter().map(|(name, id, _)| (*name, *id)).collect();
        assert_eq!(names.len(), installed.len(), "program names must be unique");
        let (user, system) = installed.split_at(programs.len());
        let state = V03State::new()
            .with_programs(user.iter().map(|(_, _, program)| program.clone()))
            .with_named_programs(system.iter().map(|(_, id, program)| (*id, program.clone())));
        let catalog = installed
            .into_iter()
            .map(|(_, id, program)| (id, program))
            .collect();
        let mut chain = Self {
            state,
            catalog,
            programs: names,
            keys: BTreeMap::new(),
            block: 1,
            now_ms: 0,
        };
        chain.set_now_ms(0);
        chain
    }

    /// The account a program named in `new` (or `CLOCK`) is deployed at.
    pub fn program(&self, name: &str) -> AccountId {
        *self
            .programs
            .get(name)
            .unwrap_or_else(|| panic!("no program named {name}"))
    }

    /// The every-block clock account's clock actor, the one `AssertTimestamp` reads.
    pub fn clock(&self) -> Actor {
        Actor::new(CLOCK_01_PROGRAM_ACCOUNT_ID, self.program(CLOCK))
    }

    /// Replaces `account` with a fresh account holding `native` of the native token.
    pub fn fund(&mut self, account: AccountId, native: u128) {
        self.state
            .force_insert_account(account, Account::funded(native));
    }

    /// A signing key and its public account, the same for the same label on every run.
    pub fn key(&mut self, label: &str) -> (PrivateKey, AccountId) {
        let key = self
            .keys
            .entry(label.to_owned())
            .or_insert_with(|| derive_key(label))
            .clone();
        let account = AccountId::from(&PublicKey::new_from_private_key(&key));
        (key, account)
    }

    /// Signs and applies one public transaction in the next block at `now_ms`.
    pub fn send(
        &mut self,
        root: Actor,
        actors: &[Actor],
        signers: &[&PrivateKey],
        msg: &impl BorshSerialize,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        self.send_with_evidence(root, actors, signers, &[], msg)
    }

    /// [`Chain::send`] carrying `evidence` that claimed fresh public accounts are public, which
    /// the sequencer needs to admit an actor on an account that does not exist yet.
    pub fn send_with_evidence(
        &mut self,
        root: Actor,
        actors: &[Actor],
        signers: &[&PrivateKey],
        evidence: &[PublicAccountEvidence],
        msg: &impl BorshSerialize,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let tx = self.transaction(root, actors, signers, evidence, msg)?;
        let result = self
            .state
            .transition_from_public_transaction(&tx, self.block, self.now_ms);
        self.block = self.block.checked_add(1).expect("block id overflow");
        result
    }

    /// Sets chain time: the next transactions run at `ms`, and the clock accounts hold `ms`,
    /// as after the sequencer's per-block clock tick (same `ClockAccountData` encoding).
    pub fn set_now_ms(&mut self, ms: u64) {
        self.now_ms = ms;
        let clock_program = self.program(CLOCK);
        let data = ClockAccountData {
            block_id: self.block,
            timestamp: ms,
        };
        for id in CLOCK_PROGRAM_ACCOUNT_IDS {
            let account = self
                .state
                .get_account_by_id(id)
                .with_actor_state(clock_program, data.to_bytes().into());
            self.state.force_insert_account(id, account);
        }
    }

    /// The raw bytes of `actor`'s state, `None` if the account holds none for that program.
    pub fn raw_state(&self, actor: Actor) -> Option<Vec<u8>> {
        self.state
            .get_account_by_id_ref(actor.account_id)?
            .data
            .actor_states
            .get(&actor.program_account_id)
            .map(|state| state.to_vec())
    }

    /// `actor`'s state decoded as borsh `T`, `None` if absent. Panics if it does not decode.
    pub fn state<T: BorshDeserialize>(&self, actor: Actor) -> Option<T> {
        self.raw_state(actor).map(|bytes| {
            borsh::from_slice(&bytes)
                .unwrap_or_else(|err| panic!("state of {actor:?} does not decode: {err}"))
        })
    }

    /// Simulates the transaction `send` would apply against current state, admitting the
    /// accounts settlement would (those that exist): cycles used and the outcome.
    pub fn cycles(
        &self,
        root: Actor,
        actors: &[Actor],
        signers: &[&PrivateKey],
        msg: &impl BorshSerialize,
    ) -> (u64, Result<(), LeeError>) {
        self.cycles_with_evidence(root, actors, signers, &[], msg)
    }

    /// [`Chain::cycles`] for a transaction carrying `evidence`; the accounts it proves public
    /// are admitted too.
    pub fn cycles_with_evidence(
        &self,
        root: Actor,
        actors: &[Actor],
        signers: &[&PrivateKey],
        evidence: &[PublicAccountEvidence],
        msg: &impl BorshSerialize,
    ) -> (u64, Result<(), LeeError>) {
        let tx = match self.transaction(root, actors, signers, evidence, msg) {
            Ok(tx) => tx,
            Err(err) => return (0, Err(err)),
        };
        let mut states = HashMap::new();
        let mut admitted: BTreeSet<AccountId> = evidence
            .iter()
            .map(PublicAccountEvidence::account_id)
            .collect();
        for actor in &tx.message().public_context.actors {
            if let Some(account) = self.state.get_account_by_id_ref(actor.account_id) {
                admitted.insert(actor.account_id);
                if let Some(state) = account.data.actor_states.get(&actor.program_account_id) {
                    states.insert(*actor, state.clone());
                }
            }
        }
        let simulation = Simulation {
            public_actor_states: states,
            admitted_accounts: Some(admitted),
        };
        lee::simulate_public_transaction(
            &tx,
            &simulation,
            &self.catalog,
            DEFAULT_PUBLIC_CYCLE_BUDGET,
        )
    }

    fn transaction(
        &self,
        root: Actor,
        actors: &[Actor],
        signers: &[&PrivateKey],
        evidence: &[PublicAccountEvidence],
        msg: &impl BorshSerialize,
    ) -> Result<PublicTransaction, LeeError> {
        let nonces = signers
            .iter()
            .map(|key| {
                let account = AccountId::from(&PublicKey::new_from_private_key(key));
                (account, self.state.get_account_by_id(account).nonce)
            })
            .collect();
        let message = Message {
            admission_evidence: evidence.to_vec(),
            ..Message::try_new(root, actors.iter().copied(), nonces, msg)?
        };
        let witness = WitnessSet::for_message(&message, signers);
        Ok(PublicTransaction::new(message, witness))
    }
}

/// Panics unless `result` is a revert whose error mentions `needle`. A guest panic surfaces as
/// `ProgramExecutionFailed("Guest panicked: <text>")`, so `needle` is the panic text.
pub fn expect_revert(result: Result<Vec<TransactionEvent>, LeeError>, needle: &str) {
    match result {
        Ok(_) => panic!("expected a revert mentioning {needle:?}, but the transaction applied"),
        Err(err) => {
            let text = format!("{err} {err:?}");
            assert!(
                text.contains(needle),
                "revert does not mention {needle:?}: {err:?}"
            );
        }
    }
}

/// SHA-256 of the label, re-hashed until it is a valid secp256k1 scalar.
fn derive_key(label: &str) -> PrivateKey {
    let mut seed: [u8; 32] = Sha256::digest(label.as_bytes()).into();
    loop {
        if let Ok(key) = PrivateKey::try_new(seed) {
            return key;
        }
        seed = Sha256::digest(seed).into();
    }
}
