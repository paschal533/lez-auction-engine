//! Spike (lez-auction-engine, 2026-10-31): call an already-deployed hello_world on LEZ v0.4.0-rc1.
//!
//! Usage: spike_call_hello <program_account_id> <target_account_id> <payer_account_id> <nofee|fee>
//!   nofee  the message as run_hello_world builds it: no fee declaration, no signatures
//!   fee    the same call with a FeeDeclaration paid and signed by the payer
//!   signed the fee call, also signed by the (fresh) target so settlement admits it
//!   sim    build the fee call but only simulate it (cycles used and revert reason); nothing sent
use std::collections::{BTreeMap, HashMap};

use common::transaction::LeeTransaction;
use lee::{
    AccountId, Actor, FeeDeclaration, PublicTransaction,
    public_transaction::{Message, WitnessSet},
};
use lee::privacy_preserving_transaction::circuit::{Dependency, ProgramCatalog, ProgramKind};
use sequencer_service_rpc::RpcClient as _;
use wallet::{WalletCore, program_facades::program_loader::ProgramLoader};

fn env_or(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn arg(n: usize) -> String {
    std::env::args().nth(n).unwrap_or_else(|| panic!("missing argument {n}"))
}

#[tokio::main]
async fn main() {
    let wallet_core = WalletCore::from_env().unwrap();
    let program: AccountId = arg(1).parse().unwrap();
    let target: AccountId = arg(2).parse().unwrap();
    let payer: AccountId = arg(3).parse().unwrap();
    let mode = arg(4);
    let with_fee = mode != "nofee";

    let greeting: Vec<u8> = b"Hola mundo!".to_vec();
    let hello = Actor::new(target, program);

    let (message, keys) = if with_fee {
        let key = wallet_core
            .storage()
            .key_chain()
            .pub_account_signing_key(payer)
            .expect("payer must be a wallet-owned public account")
            .clone();
        let nonce = wallet_core.get_accounts_nonces(&[payer]).await.unwrap()[0];
        let gas_limit = env_or("SPIKE_GAS", 200_000);
        let max_fee = u128::from(env_or("SPIKE_MAX_FEE", 10_000_000));
        println!("gas_limit {gas_limit}, max_fee {max_fee}");
        let fee = FeeDeclaration { payer, gas_limit, tip: 0, max_fee };
        let mut nonces = BTreeMap::from([(payer, nonce)]);
        let mut keys = vec![key];
        if mode == "signed" {
            let target_key = wallet_core
                .storage()
                .key_chain()
                .pub_account_signing_key(target)
                .expect("target must be a wallet-owned public account")
                .clone();
            let target_nonce = wallet_core.get_accounts_nonces(&[target]).await.unwrap()[0];
            nonces.insert(target, target_nonce);
            keys.push(target_key);
        }
        let message = Message::try_new_with_fees(hello, vec![hello], nonces, greeting, fee).unwrap();
        (message, keys)
    } else {
        (Message::try_new(hello, vec![hello], BTreeMap::new(), greeting).unwrap(), vec![])
    };

    let key_refs: Vec<_> = keys.iter().collect();
    let witness_set = WitnessSet::for_message(&message, &key_refs);
    let tx = PublicTransaction::new(message, witness_set);
    if mode == "sim" {
        let deployed = ProgramLoader(&wallet_core).program(program).await.unwrap().expect("program deployed");
        let mut programs = ProgramCatalog::default();
        programs.programs.insert(program, Dependency { program: deployed, kind: ProgramKind::Disclosed });
        let admitted = match std::env::var("SPIKE_ADMIT").as_deref() {
            Ok("payer") => Some(std::collections::BTreeSet::from([payer])),
            Ok("both") => Some(std::collections::BTreeSet::from([payer, target])),
            _ => None,
        };
        println!("admitted: {admitted:?}");
        let simulation = lee::Simulation { public_actor_states: HashMap::new(), admitted_accounts: admitted };
        let budget = tx.message().execution.fee.as_ref().map_or(1_000_000, |f| f.gas_limit);
        let (cycles, result) = lee::simulate_public_transaction(&tx, &simulation, &programs, budget);
        println!("sim: budget {budget}, cycles used {cycles}, result {result:?}");
        return;
    }
    match wallet_core.client_owned().send_transaction(LeeTransaction::Public(tx)).await {
        Ok(response) => println!("{mode}: accepted by the sequencer: {response:?}"),
        Err(err) => println!("{mode}: rejected: {err}"),
    }
}
