//! S9 spike runner (lez-auction-engine, 2026-10-31): drive s9_engine / s9_host on LEZ v0.4.0-rc1.
//!
//!   spike_s9 init-engine <engine> <config>
//!   spike_s9 init-host   <host_prog> <host> <engine> <config> <collateral>
//!   spike_s9 liquidate   <engine> <config> <host_prog> <host> <seed_byte> <amount> <evidence|noevidence> <send|sim>
//!   spike_s9 spoof       <engine> <config> <amount>          (root `Seized` to the config: must revert; sim only)
//!   spike_s9 show        <account>
//! Environment: payer `S9_PAYER` (a wallet-owned funded public account), `SPIKE_GAS` (default 1,000,000).
use std::collections::{BTreeMap, BTreeSet, HashMap};

use borsh::BorshSerialize;
use common::transaction::LeeTransaction;
use lee::{
    AccountId, Actor, FeeDeclaration, PrivateKey, PublicTransaction,
    privacy_preserving_transaction::circuit::{Dependency, ProgramCatalog, ProgramKind},
    public_transaction::{Message, WitnessSet},
    PublicAccountEvidence,
};
use lee_core::program::PdaSeed;
use sequencer_service_rpc::RpcClient as _;
use wallet::{WalletCore, program_facades::program_loader::ProgramLoader};

type Wallet = WalletCore<sequencer_service_rpc::SequencerClient>;

#[derive(BorshSerialize)]
enum EngineMsg {
    Init,
    Liquidate { host: Actor, auction_seed: [u8; 32], amount: u64 },
    Seized { amount: u64 },
}

#[derive(BorshSerialize)]
enum HostMsg {
    Init { engine: Actor, collateral: u64 },
}

struct Run {
    wallet: Wallet,
    payer: AccountId,
}

fn arg(n: usize) -> String {
    std::env::args().nth(n).unwrap_or_else(|| panic!("missing argument {n}"))
}
fn id(n: usize) -> AccountId {
    arg(n).trim_start_matches("Public/").parse().unwrap()
}

impl Run {
    fn key(&self, account: AccountId) -> PrivateKey {
        self.wallet.storage().key_chain().pub_account_signing_key(account).expect("wallet-owned account").clone()
    }

    async fn nonce(&self, account: AccountId) -> lee_core::account::Nonce {
        self.wallet.get_accounts_nonces(&[account]).await.unwrap()[0]
    }

    /// Builds a fee-paying transaction to `root`; `signers` sign besides the payer.
    async fn tx(
        &self,
        root: Actor,
        actors: Vec<Actor>,
        signers: &[AccountId],
        evidence: Vec<PublicAccountEvidence>,
        msg: impl BorshSerialize,
    ) -> PublicTransaction {
        let gas_limit = std::env::var("SPIKE_GAS").ok().and_then(|v| v.parse().ok()).unwrap_or(1_000_000);
        let fee = FeeDeclaration { payer: self.payer, gas_limit, tip: 0, max_fee: u128::from(gas_limit) * 20 + 100_000 };
        let mut nonces = BTreeMap::from([(self.payer, self.nonce(self.payer).await)]);
        let mut keys = vec![self.key(self.payer)];
        for &s in signers {
            nonces.insert(s, self.nonce(s).await);
            keys.push(self.key(s));
        }
        let mut message = Message::try_new_with_fees(root, actors, nonces, msg, fee).unwrap();
        message.admission_evidence = evidence;
        let refs: Vec<_> = keys.iter().collect();
        let witness = WitnessSet::for_message(&message, &refs);
        PublicTransaction::new(message, witness)
    }

    async fn send(&self, label: &str, tx: PublicTransaction) {
        match self.wallet.client_owned().send_transaction(LeeTransaction::Public(tx)).await {
            Ok(hash) => println!("{label}: accepted {hash:?}"),
            Err(err) => println!("{label}: rejected: {err}"),
        }
    }

    /// Simulates against the live states of every declared actor, admitting what settlement would.
    async fn sim(&self, label: &str, tx: &PublicTransaction, admitted: BTreeSet<AccountId>) {
        let mut programs = ProgramCatalog::default();
        let mut states = HashMap::new();
        for actor in &tx.message().public_context.actors {
            if !programs.programs.contains_key(&actor.program_account_id) {
                if let Some(p) = ProgramLoader(&self.wallet).program(actor.program_account_id).await.unwrap() {
                    programs.programs.insert(actor.program_account_id, Dependency { program: p, kind: ProgramKind::Disclosed });
                }
            }
            if let Some(account) = self.wallet.client_owned().get_account(actor.account_id).await.unwrap() {
                if let Some(state) = account.data.actor_states.get(&actor.program_account_id) {
                    states.insert(*actor, state.clone());
                }
            }
        }
        let budget = tx.message().execution.fee.as_ref().map_or(1_000_000, |f| f.gas_limit);
        let simulation = lee::Simulation { public_actor_states: states, admitted_accounts: Some(admitted) };
        let (cycles, result) = lee::simulate_public_transaction(tx, &simulation, &programs, budget);
        println!("{label}: sim budget {budget}, cycles {cycles}, result {result:?}");
    }
}

#[tokio::main]
async fn main() {
    let payer: AccountId = std::env::var("S9_PAYER").expect("S9_PAYER").trim_start_matches("Public/").parse().unwrap();
    let run = Run { wallet: Wallet::from_env().unwrap(), payer };
    match arg(1).as_str() {
        "init-engine" => {
            let (engine, config) = (id(2), id(3));
            let root = Actor::new(config, engine);
            let tx = run.tx(root, vec![root], &[config], vec![], EngineMsg::Init).await;
            run.send("init-engine", tx).await;
        }
        "init-host" => {
            let (host_prog, host, engine, config) = (id(2), id(3), id(4), id(5));
            let collateral: u64 = arg(6).parse().unwrap();
            let root = Actor::new(host, host_prog);
            let msg = HostMsg::Init { engine: Actor::new(config, engine), collateral };
            let tx = run.tx(root, vec![root], &[host], vec![], msg).await;
            run.send("init-host", tx).await;
        }
        "liquidate" => {
            let (engine, config, host_prog, host) = (id(2), id(3), id(4), id(5));
            let seed_byte: u8 = arg(6).parse().unwrap();
            let amount: u64 = arg(7).parse().unwrap();
            let with_evidence = arg(8) == "evidence";
            let seed = [seed_byte; 32];
            let pda = AccountId::for_public_pda(&engine, &PdaSeed::new(seed));
            let (root, host_actor, auction) =
                (Actor::new(config, engine), Actor::new(host, host_prog), Actor::new(pda, engine));
            let evidence = if with_evidence {
                vec![PublicAccountEvidence::Pda { program: engine, seed: PdaSeed::new(seed) }]
            } else {
                vec![]
            };
            let msg = EngineMsg::Liquidate { host: host_actor, auction_seed: seed, amount };
            let tx = run.tx(root, vec![root, host_actor, auction], &[], evidence, msg).await;
            let label = format!("liquidate({}, seed {seed_byte}, auction {pda})", arg(8));
            let mut admitted = BTreeSet::from([payer, config, host]);
            if with_evidence {
                admitted.insert(pda);
            }
            if arg(9) == "sim" {
                run.sim(&label, &tx, admitted).await;
            } else {
                run.send(&label, tx).await;
            }
        }
        "spoof" => {
            let (engine, config) = (id(2), id(3));
            let amount: u64 = arg(4).parse().unwrap();
            let root = Actor::new(config, engine);
            let tx = run.tx(root, vec![root], &[], vec![], EngineMsg::Seized { amount }).await;
            run.sim("spoof Seized from the root", &tx, BTreeSet::from([payer, config])).await;
        }
        "show" => {
            let account = id(2);
            println!("{account}: {:?}", run.wallet.client_owned().get_account(account).await.unwrap());
        }
        other => panic!("unknown command {other}"),
    }
}
