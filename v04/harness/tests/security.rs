//! Security tests in actor form (design note §4.3 c, the R1 guards; v0.2.4 counterpart
//! `programs/integration_tests/tests/security.rs`).
//!
//! The attacker's code is the `impostor` guest (`harness::impostor`): it sends any bytes it is
//! given to any actor, as the actor `(account, Impostor)` it is deployed at. The tests cover:
//! - guard 3, the program half of every sender check: forged `Seized`, `Reserve`, `Release`,
//!   `Settled`, `BidCtx`, `Open`, `Bump`, `Finish`, … from `(config, Impostor)` and from impostors
//!   at the other engine and host accounts;
//! - guard 3, the account half: what engine actors at attacker accounts can and cannot address to
//!   a victim (see `engine_actor_at_an_attacker_account_cannot_either`);
//! - S9 / ADR-027: a rogue config of the same engine cannot make the bound host seize;
//! - guard 2: internal messages sent as root messages.
//!
//! `security_hosted.rs` holds the tests with an impostor bound as the host (a `Seized` outside
//! `AwaitingSeize`, a host that never replies).
//!
//! Every revert names the engine's exact error text (`fail_text`) or the token's panic text; the
//! state the forged message aimed at is compared before and after.

use auction_engine_params::EngineError;
use engine_core::{
    actors::{
        accounting_actor, auction_actor, collateral_actor, config_actor, record_actor,
        vault_holding,
    },
    check::fail_text,
    msg::EngineMsg,
    state::{AccountingState, CollateralState, ConfigState, LiquidationRecordState},
};
use harness::{
    World, expect_revert,
    impostor::{ImpostorMsg, encode},
    invariants,
    world::{RAY, SALT},
};
use lee::{AccountId, Actor, PublicAccountEvidence};
use token_core::TokenMsg;

const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
const TARGET: u128 = 660_000;
/// The chain time of every test's liquidation, ms.
const T: u64 = 1_000_000;
/// The rogue config's scripted seizure: 3 collateral (all of it the keeper reward) against this
/// debt, so its auction's target and reservation are `ceil(10_000 × 1.1)`.
const ROGUE_DEBT: u128 = 10_000;
const ROGUE_TARGET: u128 = 11_000;

/// `World::new()`, one unsafe 500_000 / 600_000 position at `T` (price 0.9), liquidated as
/// auction 0: the auction is `Auctioning` with the 660_000 reservation booked.
fn liquidated() -> (World, AccountId) {
    let mut w = World::new();
    w.set_now_ms(T);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    w.liquidate(position, 0).expect("the keeper liquidates");
    invariants::check(&w);
    (w, position)
}

fn collateral_state(w: &World) -> CollateralState {
    w.chain
        .raw_state(w.collateral)
        .and_then(|bytes| CollateralState::decode(&bytes))
        .expect("a collateral state")
}

/// Every state a forged message could aim at in the liquidated world: the engine's config,
/// collateral, accounting, record, auction, the auction vault, the keeper's and the owner's
/// holdings, the host position and its vault, and the stablecoin definition.
fn snapshot(w: &World, position: AccountId) -> Vec<Option<Vec<u8>>> {
    let owner = w.owner_of(position).account;
    [
        w.config,
        w.collateral,
        w.accounting,
        w.record(position),
        w.auction(position, 0),
        w.vault(position, 0),
        Actor::new(w.keeper, w.token),
        Actor::new(owner, w.token),
        Actor::new(position, w.host_prog),
        w.host_vault(position),
        Actor::new(w.stable_def, w.token),
    ]
    .into_iter()
    .map(|actor| w.chain.raw_state(actor))
    .collect()
}

/// The internal messages an attacker would forge against auction 0 of `position`, each with a
/// payload that would hurt if it were accepted.
struct Forged {
    seized: EngineMsg,
    reserve: EngineMsg,
    release: EngineMsg,
    settled: EngineMsg,
    bid_ctx: EngineMsg,
    open: EngineMsg,
    bump: EngineMsg,
    liq_ctx: EngineMsg,
    configure: EngineMsg,
    settle_empty: EngineMsg,
}

impl Forged {
    fn new(w: &World, position: AccountId) -> Self {
        let collateral = collateral_state(w);
        Self {
            // Rebook the auction with a tiny lot.
            seized: EngineMsg::Seized {
                position,
                owner: w.keeper,
                collateral: 1,
                normalized_debt: 1,
                nominal_debt: 1,
            },
            // Exhaust the caps.
            reserve: EngineMsg::Reserve {
                position,
                nonce: 0,
                amount: u128::MAX / 2,
            },
            // Free the live auction's reservation.
            release: EngineMsg::Release {
                position,
                nonce: 0,
                amount: TARGET,
            },
            // Book a shortfall that never happened.
            settled: EngineMsg::Settled {
                config: w.config.account_id,
                position,
                nonce: 0,
                debt_raised: 0,
                penalty_raised: 0,
                shortfall: 1_000_000,
                release: TARGET,
                leftover: 0,
            },
            // A bid at an absurd price: the whole lot for one unit.
            bid_ctx: EngineMsg::BidCtx {
                bidder: w.keeper,
                position,
                nonce: 0,
                max_debt_in: 1,
                min_collateral_out: 0,
                price_ray: RAY * 1_000_000,
                now_ms: T,
                paused_total_ms: 0,
                min_bid: 0,
                chost: 0,
            },
            // Re-open the live auction with the attacker as keeper and host.
            open: EngineMsg::Open {
                position,
                nonce: 0,
                keeper: w.keeper,
                host: w.publisher,
                token: w.token,
                stable_def: w.stable_def,
                now_ms: T,
                price_ray: RAY,
                collateral: Box::new(collateral.clone()),
                global_cap: u128::MAX,
                global_reserved: 0,
            },
            // Rewind the record.
            bump: EngineMsg::Bump { position, nonce: 1 },
            // Liquidate without an observation.
            liq_ctx: EngineMsg::LiqCtx {
                position,
                nonce: 1,
                keeper: w.keeper,
                now_ms: T,
                price_ray: RAY,
                collateral: Box::new(collateral.clone()),
            },
            // Rewrite the type's parameters.
            configure: EngineMsg::Configure {
                collateral_def: w.collateral_def,
                args: World::collateral_args(),
                ctx: collateral.ctx,
            },
            settle_empty: EngineMsg::SettleEmpty {
                config: w.config.account_id,
                position,
                nonce: 0,
            },
        }
    }
}

/// Each `(from, to, message, expected panic text)` must revert with exactly that text, and the
/// liquidated world must be unchanged afterwards.
fn expect_all_refused(
    w: &mut World,
    position: AccountId,
    cases: Vec<(Actor, Actor, Vec<u8>, String)>,
) {
    let before = snapshot(w, position);
    for (from, to, message, needle) in cases {
        let forward = ImpostorMsg::Forward { to, message };
        expect_revert(w.chain.send(from, &[from, to], &[], &forward), &needle);
    }
    assert_eq!(
        snapshot(w, position),
        before,
        "no forged message changed any state"
    );
    invariants::check(w);
}

/// Guard 3 from `(K, Impostor)`: the impostor shares the config's account, so only the program
/// half of the sender differs; every internal handler compares the whole actor.
#[test]
fn impostor_program_at_the_config_account_cannot_send_internal_messages() {
    let (mut w, position) = liquidated();
    let imp = w.impostor(w.config.account_id);
    let f = Forged::new(&w, position);
    let (auction, record) = (w.auction(position, 0), w.record(position));
    let cases = vec![
        (
            auction,
            encode(&f.seized),
            fail_text(EngineError::HostMismatch),
        ),
        (
            w.collateral,
            encode(&f.reserve),
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            w.config,
            encode(&f.reserve),
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            w.collateral,
            encode(&f.release),
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            w.config,
            encode(&f.release),
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            w.accounting,
            encode(&f.settled),
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            auction,
            encode(&f.bid_ctx),
            fail_text(EngineError::WrongAccount("coll_config")),
        ),
        (
            auction,
            encode(&f.open),
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            record,
            encode(&f.bump),
            fail_text(EngineError::WrongAccount("record")),
        ),
        (
            auction,
            encode(&EngineMsg::Finish),
            fail_text(EngineError::WrongAccount("finish")),
        ),
        (
            auction,
            encode(&f.settle_empty),
            fail_text(EngineError::WrongAccount("config")),
        ),
        (
            w.config,
            encode(&f.liq_ctx),
            fail_text(EngineError::WrongAccount("coll_config")),
        ),
        (
            w.collateral,
            encode(&f.configure),
            fail_text(EngineError::NotAdmin),
        ),
        (
            w.collateral,
            encode(&EngineMsg::Observe { now_ms: T }),
            fail_text(EngineError::NotAdmin),
        ),
        (
            w.collateral,
            encode(&EngineMsg::Proceed),
            fail_text(EngineError::WrongAccount("proceed")),
        ),
    ];
    let mut cases: Vec<_> = cases
        .into_iter()
        .map(|(to, message, needle)| (imp, to, message, needle))
        .collect();
    // `Mint`: the stablecoin's only minter is the host's params actor, not anything at `K`.
    let mint = TokenMsg::MintTo {
        to: w.keeper,
        amount: 1_000_000,
    };
    let sdef = Actor::new(w.stable_def, w.token);
    cases.push((imp, sdef, encode(&mint), "token: NotMinter".to_owned()));
    expect_all_refused(&mut w, position, cases);
}

/// Guard 3 from impostors at the collateral actor's, the auction's and the host position's
/// accounts: right account, wrong program, each sending what the real actor there may send.
#[test]
fn impostors_at_other_engine_and_host_accounts_cannot_either() {
    let (mut w, position) = liquidated();
    let f = Forged::new(&w, position);
    let auction = w.auction(position, 0);
    let at_collateral = w.impostor(w.collateral.account_id);
    let at_auction = w.impostor(auction.account_id);
    let at_position = w.impostor(position);
    // Drain the auction vault: the token wants the VAULT seed, which only the engine's auction
    // actor can attach.
    let drain = TokenMsg::Transfer {
        definition: w.collateral_def,
        to: w.keeper,
        amount: COLLATERAL - 45,
    };
    let cases = vec![
        (
            at_collateral,
            auction,
            encode(&f.bid_ctx),
            fail_text(EngineError::WrongAccount("coll_config")),
        ),
        (
            at_collateral,
            w.config,
            encode(&f.liq_ctx),
            fail_text(EngineError::WrongAccount("coll_config")),
        ),
        (
            at_collateral,
            w.config,
            encode(&f.reserve),
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            at_auction,
            w.collateral,
            encode(&f.reserve),
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            at_auction,
            w.config,
            encode(&f.release),
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            at_auction,
            w.collateral,
            encode(&f.release),
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            at_auction,
            w.accounting,
            encode(&f.settled),
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            at_auction,
            w.vault(position, 0),
            encode(&drain),
            "token: Unauthorized".to_owned(),
        ),
        (
            at_position,
            auction,
            encode(&f.seized),
            fail_text(EngineError::HostMismatch),
        ),
    ];
    expect_all_refused(&mut w, position, cases);
}

/// Guard 3, the account half, from engine actors at attacker accounts (ruling R-10a). An
/// attacker's engine actors are its proxy `(attacker, E)` and everything under its own config
/// `K' = config_actor(E, attacker, SALT)`: `K'`, its collateral actor `C'`, its accounting,
/// records and auctions. They run only engine code, so they send only what the engine sends, and
/// the engine derives every internal target from the sender's own config:
/// - `K'` sends `Bump` / `Open` to `record_actor` / `auction_actor(E, K', ..)`;
/// - an auction `A'` of `K'` sends `Reserve`, `Settled` and `Release` to
///   `collateral_actor` / `accounting_actor(E, a.config, ..)` and `(a.config, E)`, where `a.config`
///   is the sender of its `Open`, i.e. `K'`;
/// - `C'` sends `BidCtx` (behind its auction probe) to `auction_actor(E, K', ..)`.
///
/// So no engine path addresses `Reserve`, `Release`, `Settled`, `BidCtx`, `Open` or `Bump` from an
/// attacker account to a victim actor; the account half of those sender checks is defence in
/// depth that is unreachable here (the program half is exercised by the impostor tests). The only
/// engine messages from attacker accounts that do reach victim actors are:
/// 1. the proxy's forwards, whose target the root message names: `ClaimLeftover` to the victim's
///    auction is refused with `NotOwner` (`Liquidate` there is the permissionless keeper path);
/// 2. the Read-block messages to the actors a rogue `Init` binds as host, oracle and clock: a
///    rogue config naming the victim's collateral actor as its oracle delivers its `ReadState`
///    there from `(K'', E)`, and the victim refuses it as no engine message.
///
/// The test runs both, then drives every emitter above for real (a full liquidation of a scripted
/// position under `K'` bound to an impostor host, a `Bid` at `C'` naming the victim's position,
/// and `SettleEmpty` of the auction of `K'`), and shows that the messages land on the attacker's
/// own actors while the victim's engine state stays byte-identical.
#[test]
fn engine_actor_at_an_attacker_account_cannot_either() {
    let (mut w, position) = liquidated();
    let victim = victim_engine_state(&w, position);
    let (attacker_key, attacker) = w.chain.key("attacker");
    w.chain.fund(attacker, 1);

    // 1. The proxy's one forward to a victim actor that is not the keeper path.
    expect_revert(
        w.claim_as(attacker, &[&attacker_key], position, 0),
        &fail_text(EngineError::NotOwner),
    );
    // 2. A rogue config's oracle read, delivered to the victim's collateral actor.
    let mut init = w.init_msg();
    if let EngineMsg::Init { salt, oracle, .. } = &mut init {
        *salt = SALT + 1;
        *oracle = w.collateral;
    }
    expect_revert(
        w.send_engine(attacker, &[&attacker_key], &init),
        "engine: BadMessage",
    );
    assert_eq!(victim_engine_state(&w, position), victim);
    // Control: the same rogue `Init` with the real oracle goes through, so the refusal above
    // came from the victim actor the read was addressed to.
    if let EngineMsg::Init { oracle, .. } = &mut init {
        *oracle = w.oracle;
    }
    w.send_engine(attacker, &[&attacker_key], &init)
        .expect("the same rogue Init with the real oracle");

    // 3. Every internal emitter under K'. Its liquidation sends `Bump`, `Open`, then (from its
    //    auction) `Reserve` to C' and K'.
    let (e, def) = (w.engine, w.collateral_def);
    let rogue = config_actor(e, attacker, SALT).account_id;
    let rogue_position = w.scripted_position("rogue-position", rogue, 3, ROGUE_DEBT);
    let host = w.impostor_host("rogue-host", Some(rogue_position));
    w.configure_with_host(attacker, &attacker_key, host);
    let p = rogue_position.account_id;
    w.fund_vault(rogue, p, 3);
    w.liquidate_through(rogue, host, rogue_position)
        .expect("K' liquidates into its own auction");
    let rogue_config = Actor::new(rogue, e);
    let rogue_collateral = collateral_actor(e, rogue, def);
    let rogue_auction = auction_actor(e, rogue, p, 0);
    let reserved = |w: &World| {
        (
            decode_state(w, rogue_collateral, CollateralState::decode).type_reserved,
            decode_state(w, rogue_config, ConfigState::decode).global_reserved,
        )
    };
    assert_eq!(reserved(&w), (ROGUE_TARGET, ROGUE_TARGET), "the caps of K'");
    let record = decode_state(
        &w,
        record_actor(e, rogue, p),
        LiquidationRecordState::decode,
    );
    assert_eq!(record.count, 1, "the record of K'");
    assert_eq!(victim_engine_state(&w, position), victim);

    // `BidCtx`: a bid at C' naming the victim's position goes to the auction of K' for that
    // position, which does not exist; the victim's auction is declared and untouched.
    let bid = EngineMsg::Bid {
        bidder: attacker,
        config: rogue,
        collateral_def: def,
        position,
        nonce: 0,
        max_debt_in: 1,
        min_collateral_out: 0,
        now_ms: T,
    };
    let actors = [
        rogue_collateral,
        host,
        w.oracle,
        w.chain.clock(),
        auction_actor(e, rogue, position, 0),
        w.auction(position, 0),
    ];
    expect_revert(
        w.chain.send(rogue_collateral, &actors, &[], &bid),
        &fail_text(EngineError::WrongAccount("auction")),
    );

    // `Settled` and `Release`: the auction of K' has an empty lot (the reward took all 3), so
    // `SettleEmpty` ends it, and its chain end goes to the accounting of K', to C' and to K'.
    let settle = EngineMsg::SettleEmpty {
        config: rogue,
        position: p,
        nonce: 0,
    };
    let rogue_accounting = accounting_actor(e, rogue);
    let actors = [
        rogue_config,
        rogue_auction,
        rogue_accounting,
        rogue_collateral,
    ];
    w.chain
        .send(rogue_config, &actors, &[], &settle)
        .expect("K' settles its own auction");
    assert_eq!(reserved(&w), (0, 0), "released at the actors of K'");
    let books = decode_state(&w, rogue_accounting, AccountingState::decode);
    assert_eq!(
        books.unbacked_debt, ROGUE_DEBT,
        "booked by the accounting of K'"
    );
    assert_eq!(victim_engine_state(&w, position), victim);
    invariants::check(&w);
}

/// The victim engine's own states: config, collateral, accounting, and `position`'s record,
/// auction 0 and its vault.
fn victim_engine_state(w: &World, position: AccountId) -> Vec<Option<Vec<u8>>> {
    [
        w.config,
        w.collateral,
        w.accounting,
        w.record(position),
        w.auction(position, 0),
        w.vault(position, 0),
    ]
    .into_iter()
    .map(|actor| w.chain.raw_state(actor))
    .collect()
}

/// `actor`'s state decoded with `decode`; panics if it is absent or not that state.
fn decode_state<S>(w: &World, actor: Actor, decode: fn(&[u8]) -> Option<S>) -> S {
    w.chain
        .raw_state(actor)
        .and_then(|bytes| decode(&bytes))
        .unwrap_or_else(|| panic!("no expected state at {actor:?}"))
}

/// S9 / ADR-027 through the real engine: the attacker creates its own config of the same engine
/// (same salt; the admin is in the seed, so it is a different actor) with the real host, and a
/// keeper liquidates the victim's position through it. The host takes `LiquidatePosition` only
/// from the config it is bound to, so the whole transaction reverts and nothing moves; the honest
/// liquidation still works afterwards.
#[test]
fn rogue_config_of_the_same_engine_cannot_seize() {
    let mut w = World::new();
    w.set_now_ms(T);
    let position = w.open_unsafe_position(COLLATERAL, DEBT);
    let (attacker_key, attacker) = w.chain.key("attacker");
    w.chain.fund(attacker, 1);
    let rogue = config_actor(w.engine, attacker, SALT);
    assert_ne!(rogue, w.config, "the admin is in the config seed");
    let init = w.init_msg();
    w.send_engine(attacker, &[&attacker_key], &init)
        .expect("anyone can create a config of the engine");
    let set = w.set_collateral_msg(World::collateral_args());
    w.send_engine(attacker, &[&attacker_key], &set)
        .expect("the rogue config configures the collateral");

    let (e, r) = (w.engine, rogue.account_id);
    let rogue_auction = auction_actor(e, r, position, 0);
    let rogue_record = record_actor(e, r, position);
    let keeper_root = Actor::new(w.keeper, e);
    let msg = EngineMsg::Liquidate {
        config: r,
        collateral_def: w.collateral_def,
        position,
        nonce: 0,
        now_ms: T,
    };
    let actors = [
        keeper_root,
        collateral_actor(e, r, w.collateral_def),
        w.params,
        w.oracle,
        w.chain.clock(),
        rogue,
        rogue_record,
        rogue_auction,
        Actor::new(position, w.host_prog),
        w.host_vault(position),
        vault_holding(e, w.token, rogue_auction.account_id),
        Actor::new(w.keeper, w.token),
    ];
    let evidence = [PublicAccountEvidence::Pda {
        program: e,
        seed: engine_core::actors::vault_seed(rogue_auction.account_id),
    }];
    let before = snapshot_host(&w, position);
    let keeper_key = w.keeper_key.clone();
    expect_revert(
        w.chain
            .send_with_evidence(keeper_root, &actors, &[&keeper_key], &evidence, &msg),
        "host: NotEngine",
    );
    assert_eq!(
        snapshot_host(&w, position),
        before,
        "the position is untouched"
    );
    assert_eq!(w.chain.raw_state(rogue_auction), None);
    assert_eq!(w.chain.raw_state(rogue_record), None);
    invariants::check(&w);

    w.liquidate(position, 0)
        .expect("the bound config still liquidates");
    invariants::check(&w);
}

/// The host position, its vault and the keeper's holding.
fn snapshot_host(w: &World, position: AccountId) -> Vec<Option<Vec<u8>>> {
    [
        Actor::new(position, w.host_prog),
        w.host_vault(position),
        Actor::new(w.keeper, w.token),
    ]
    .into_iter()
    .map(|actor| w.chain.raw_state(actor))
    .collect()
}

/// Root messages need no relay at all: every internal message sent as the transaction's root
/// straight at the engine actor it is meant for has no sender, and is refused (guard 2: "any
/// other message, including a root message, panics").
#[test]
fn root_messages_to_internal_actors_revert() {
    let (mut w, position) = liquidated();
    let f = Forged::new(&w, position);
    let (auction, record) = (w.auction(position, 0), w.record(position));
    let cases = [
        (auction, f.seized, fail_text(EngineError::HostMismatch)),
        (
            w.collateral,
            f.reserve.clone(),
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            w.config,
            f.reserve,
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            w.collateral,
            f.release.clone(),
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            w.config,
            f.release,
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            w.accounting,
            f.settled,
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            auction,
            f.bid_ctx,
            fail_text(EngineError::WrongAccount("coll_config")),
        ),
        (
            auction,
            f.open,
            fail_text(EngineError::WrongAccount("auction")),
        ),
        (
            record,
            f.bump,
            fail_text(EngineError::WrongAccount("record")),
        ),
        (
            auction,
            EngineMsg::Finish,
            fail_text(EngineError::WrongAccount("finish")),
        ),
        (
            auction,
            f.settle_empty,
            fail_text(EngineError::WrongAccount("config")),
        ),
        (
            w.config,
            f.liq_ctx,
            fail_text(EngineError::WrongAccount("coll_config")),
        ),
        (w.collateral, f.configure, fail_text(EngineError::NotAdmin)),
        (
            w.collateral,
            EngineMsg::Observe { now_ms: T },
            fail_text(EngineError::NotAdmin),
        ),
        (
            w.config,
            EngineMsg::InitReads,
            fail_text(EngineError::NotAdmin),
        ),
        (
            w.collateral,
            EngineMsg::Proceed,
            fail_text(EngineError::WrongAccount("proceed")),
        ),
        (
            w.config,
            EngineMsg::Proceed,
            fail_text(EngineError::WrongAccount("proceed")),
        ),
    ];
    let before = snapshot(&w, position);
    for (to, msg, needle) in cases {
        expect_revert(w.chain.send(to, &[to], &[], &msg), &needle);
    }
    assert_eq!(snapshot(&w, position), before);
    invariants::check(&w);
}
