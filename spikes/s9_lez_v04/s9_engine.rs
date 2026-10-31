use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, Actor},
    program::{PdaSeed, ReceiveInput, Response, SyncMessage},
};

// S9 spike (lez-auction-engine): the engine side of a liquidation conversation on LEZ 0.4.
//
// 1. `Liquidate` at the engine's config actor records what it expects, then asks the host to seize.
// 2. The host replies `Seized`; the config actor accepts it only from exactly the host actor it asked
//    (whole-actor check, not just the program) and only for the amount it asked for.
// 3. The config actor opens a fresh auction PDA (seed from the caller) by messaging it with the seed
//    granted; the PDA accepts `Open` only from exactly its config actor.
//
// Message layouts are shared with s9_host.rs by variant index (borsh), as a real interface would be.

#[derive(BorshSerialize, BorshDeserialize)]
pub enum EngineMsg {
    Init,
    Liquidate { host: Actor, auction_seed: [u8; 32], amount: u64 },
    Seized { amount: u64 },
    Open { config: Actor, amount: u64 },
}

#[derive(BorshSerialize, BorshDeserialize, Default)]
struct Config {
    initialised: bool,
    pending: Option<Pending>,
    liquidations: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Copy)]
struct Pending {
    host: Actor,
    auction_seed: [u8; 32],
    amount: u64,
}

#[derive(BorshSerialize, BorshDeserialize)]
struct Auction {
    config: Actor,
    amount: u64,
}

// Host message `Seize` (s9_host.rs `HostMsg` variant 1).
#[derive(BorshSerialize)]
enum HostMsg {
    _Init,
    Seize { amount: u64 },
}

lee_core::actor_entrypoint!(handle_message);

fn load<T: BorshDeserialize + Default>(input: &ReceiveInput) -> T {
    if input.pre_state.as_ref().is_empty() {
        T::default()
    } else {
        T::try_from_slice(input.pre_state.as_ref()).expect("state decodes")
    }
}

fn store<T: BorshSerialize>(value: &T) -> Response {
    Response::set_state(borsh::to_vec(value).expect("state encodes"))
}

fn handle_message(input: &ReceiveInput, message: EngineMsg) -> Response {
    match message {
        EngineMsg::Init => {
            assert!(input.is_authorized, "init: the config account must sign");
            assert!(input.pre_state.as_ref().is_empty(), "init: already initialised");
            store(&Config { initialised: true, ..Config::default() })
        }
        EngineMsg::Liquidate { host, auction_seed, amount } => {
            let mut config: Config = load(input);
            assert!(config.initialised, "liquidate: not a config");
            assert!(config.pending.is_none(), "liquidate: a conversation is already open");
            assert!(input.from.is_none(), "liquidate: only as the transaction's root");
            config.pending = Some(Pending { host, auction_seed, amount });
            store(&config).send_sync(host, &HostMsg::Seize { amount })
        }
        EngineMsg::Seized { amount } => {
            let mut config: Config = load(input);
            let pending = config.pending.take().expect("seized: nothing pending");
            assert_eq!(input.from, Some(pending.host), "seized: not from the host that was asked");
            assert_eq!(amount, pending.amount, "seized: amount differs from the request");
            config.liquidations += 1;
            let seed = PdaSeed::new(pending.auction_seed);
            let program = input.receiver.program_account_id;
            let auction = Actor::new(AccountId::for_public_pda(&program, &seed), program);
            store(&config).send(
                SyncMessage::new(auction, &EngineMsg::Open { config: input.receiver, amount })
                    .with_pda_seeds(vec![seed]),
            )
        }
        EngineMsg::Open { config, amount } => {
            assert_eq!(input.from, Some(config), "open: not from its config actor");
            assert!(input.is_authorized, "open: the PDA seed was not granted");
            assert!(input.pre_state.as_ref().is_empty(), "open: auction exists");
            store(&Auction { config, amount })
        }
    }
}
