//! The system instructions end to end on a live LEZ v0.4.0-rc1 sequencer, through the SDK (Plan 6
//! Task 9): `Init` with the protocol token enabled (genesis to the taker) → `SetCollateral` → a
//! lot-3 position liquidated and settled empty (a shortfall `U`) → `StartDebt` (the buffer
//! holding still absent: the SDK attaches its `Pda` evidence) → `TakeDebt` of the whole lot →
//! `U − 500` stablecoin donated to the buffer → `SettleDebt u128::MAX` → 1_500 donated →
//! `StartSurplus` → `TakeSurplus` of the whole lot → `CheckSolvency` (`Healthy`).
//!
//! Prints each transaction's hash, simulated cycles, declared `gas_limit` and the fee charged,
//! and asserts the end state: `unbacked_debt` 0, both auctions `Ended`, the host not frozen, and
//! the protocol supply equal to genesis + minted − burned (all of it in the taker's holding).
//!
//! Driven by `v04/scripts/e2e.sh` (stage `system`, after `slice`), with the environment of
//! `examples/slice.rs`: `SEQUENCER_URL`, the five program accounts, `PAYER`, optional `RUN_TAG`
//! and `SLACK_MS`. Its keys derive from `RUN_TAG` + `/system`, so it never collides with the
//! slice's config on the same sequencer.
//!
//! The live plumbing (session, signers, deploy of token/oracle/host) is a trimmed copy of
//! `slice.rs`'s: an example is its own crate, and a shared module would leave each example with
//! unused items under `-D warnings`.

use std::{collections::BTreeMap, env, error::Error, str::FromStr, time::Duration};

use auction_engine_params::args::{CollateralArgs, EngineParamsArgs};
use borsh::BorshSerialize;
use clock_core::{CLOCK_01_PROGRAM_ACCOUNT_ID, ClockAccountData};
use common::HashType;
use engine_core::{
    msg::ProtocolGenesis,
    state::AccountingState,
    system_state::{DebtAuctionState, SurplusAuctionState, SystemPhase},
};
use lee::{AccountId, Actor, PrivateKey, PublicAccountEvidence, PublicKey};
use mock_host_core::{HostMsg, Params};
use oracle_core::OracleMsg;
use sdk::{Deployment, InitArgs, LiquidateArgs, Plan, Sent, Snapshot, StateSource, TakeArgs};
use sequencer_service_rpc::{RpcClient as _, SequencerClient, SequencerClientBuilder};
use sha2::{Digest as _, Sha256};
use token_core::{Definition, Holding, TokenMsg, holding_actor};

type Res<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// 1e27.
const RAY: u128 = 1_000_000_000_000_000_000_000_000_000;
/// The mock host's minimum collateral ratio, 150 %; its redemption price and the oracle, 0.5.
const MIN_CR_RAY: u128 = RAY * 3 / 2;
const PRICE_RAY: u128 = RAY / 2;
const SALT: u64 = 1;
const WHALE_COLLATERAL: u128 = 1_000_000_000_000;
/// Plan 5's lot-3 `SettleEmpty` case (`World::book_shortfall`): the keeper reward takes the whole
/// 3-unit seizure, so the empty lot books the debt as a shortfall.
const SHORTFALL_COLLATERAL: u128 = 3;
const SHORTFALL_DEBT: u128 = 10_000;
/// The protocol token's genesis supply, credited to the taker at `Init`.
const GENESIS: u128 = 10_000;
/// The second donation: `surplus_buffer_target` 1_000 + `surplus_lot` 500 of the test defaults.
const SURPLUS_DONATION: u128 = 1_500;
/// The stablecoin the donor mints: `U − debt_lot` for the settle, then [`SURPLUS_DONATION`].
const DONOR_FUNDS: u128 = 20_000;
/// See `slice.rs`: a 15 s-block sequencer needs >= 60 s of clock slack (ADR-058).
const DEFAULT_SLACK_MS: u64 = 60_000;
const POLL: Duration = Duration::from_secs(1);
const INCLUSION_TIMEOUT: Duration = Duration::from_secs(180);

/// The relay guest's root message: forward `message` to `to`.
#[derive(BorshSerialize)]
struct Forward {
    to: Actor,
    message: Vec<u8>,
}

/// The deployed programs.
struct Programs {
    token: AccountId,
    oracle: AccountId,
    host: AccountId,
    relay: AccountId,
    engine: AccountId,
}

/// One signer: its key and account.
#[derive(Clone)]
struct Signer {
    key: PrivateKey,
    account: AccountId,
}

/// One sent transaction, as the summary table prints it.
struct Row {
    label: String,
    sent: Sent,
    block: u64,
    fee: u128,
}

/// The live session: sequencer, wallet, payer, signers and the transactions sent so far.
struct Live {
    client: SequencerClient,
    wallet: sdk::live::Wallet,
    payer: AccountId,
    tag: String,
    keys: BTreeMap<AccountId, PrivateKey>,
    rows: Vec<Row>,
}

fn env_account(name: &str) -> Res<AccountId> {
    let raw = env::var(name).map_err(|_| format!("{name} is not set"))?;
    let raw = raw.trim().trim_start_matches("Public/");
    AccountId::from_str(raw).map_err(|err| format!("{name}={raw}: {err:?}").into())
}

/// SHA-256 of `tag/label`, re-hashed until it is a valid secp256k1 scalar.
fn derive_key(tag: &str, label: &str) -> PrivateKey {
    let mut seed: [u8; 32] = Sha256::digest(format!("{tag}/{label}").as_bytes()).into();
    loop {
        if let Ok(key) = PrivateKey::try_new(seed) {
            return key;
        }
        seed = Sha256::digest(seed).into();
    }
}

/// A plan for a non-engine message (token, oracle, host, relay).
fn plan(
    root: Actor,
    actors: Vec<Actor>,
    signers: Vec<AccountId>,
    evidence: Vec<PublicAccountEvidence>,
    msg: &impl BorshSerialize,
) -> Res<Plan> {
    Ok(Plan {
        root,
        actors,
        signers,
        evidence,
        message: borsh::to_vec(msg)?,
    })
}

impl Live {
    async fn connect() -> Res<Self> {
        let url = env::var("SEQUENCER_URL").unwrap_or_else(|_| "http://127.0.0.1:3040".into());
        let client = SequencerClientBuilder::default().build(url.as_str())?;
        let wallet = sdk::live::Wallet::from_env_with_client(client.clone())?;
        let payer = env_account("PAYER")?;
        let payer_key = wallet
            .storage()
            .key_chain()
            .pub_account_signing_key(payer)
            .cloned()
            .ok_or_else(|| format!("the wallet holds no key for the payer {payer}"))?;
        let tag = format!(
            "{}/system",
            env::var("RUN_TAG").unwrap_or_else(|_| "e2e".into())
        );
        Ok(Self {
            client,
            wallet,
            payer,
            tag,
            keys: BTreeMap::from([(payer, payer_key)]),
            rows: Vec::new(),
        })
    }

    fn signer(&mut self, label: &str) -> Signer {
        let key = derive_key(&self.tag, label);
        let account = AccountId::from(&PublicKey::new_from_private_key(&key));
        self.keys.insert(account, key.clone());
        Signer { key, account }
    }

    async fn snapshot(&self, accounts: &[AccountId]) -> Res<Snapshot> {
        Ok(Snapshot::fetch(&self.client, accounts.iter().copied()).await?)
    }

    async fn raw_state(&self, actor: Actor) -> Res<Option<Vec<u8>>> {
        Ok(self.snapshot(&[actor.account_id]).await?.actor_state(actor))
    }

    /// `Key` evidence for `signer`'s account while it does not exist.
    async fn key_evidence_if_fresh(&self, signer: &Signer) -> Res<Vec<PublicAccountEvidence>> {
        let snapshot = self.snapshot(&[signer.account]).await?;
        Ok(if snapshot.account_exists(signer.account) {
            Vec::new()
        } else {
            vec![PublicAccountEvidence::Key(PublicKey::new_from_private_key(
                &signer.key,
            ))]
        })
    }

    /// The every-block clock's timestamp at the head, ms: the time claims are made at.
    async fn now_ms(&self) -> Res<u64> {
        let clock = Actor::new(CLOCK_01_PROGRAM_ACCOUNT_ID, clock_core::clock_account_id());
        let bytes = self
            .raw_state(clock)
            .await?
            .ok_or("the clock account holds no clock state: wrong clock program id?")?;
        Ok(ClockAccountData::from_bytes(&bytes).timestamp)
    }

    /// Simulates, fits, sends and confirms `plan`, then records the fee the payer was charged.
    async fn run(&mut self, label: &str, plan: &Plan) -> Res<()> {
        let before = self.client.get_account_balance(self.payer).await?;
        let keys: Vec<&PrivateKey> = self.keys.values().collect();
        let sent = sdk::send_signed(&self.client, &self.wallet, plan, self.payer, &keys)
            .await
            .map_err(|err| format!("{label}: {err}"))?;
        let block = sdk::confirm(&self.client, sent.hash, POLL, INCLUSION_TIMEOUT)
            .await
            .map_err(|err| format!("{label}: {err}"))?;
        let after = self.client.get_account_balance(self.payer).await?;
        let fee = before
            .checked_sub(after)
            .ok_or_else(|| format!("{label}: the payer's balance rose"))?;
        if fee > sent.max_fee {
            return Err(format!("{label}: charged {fee} above max_fee {}", sent.max_fee).into());
        }
        println!(
            "{label:<26} tx {} block {block:>4} cycles {:>9} gas_limit {:>9} max_fee {:>9} fee {fee:>9}",
            HashType(sent.hash),
            sent.cycles,
            sent.gas_limit,
            sent.max_fee,
        );
        self.rows.push(Row {
            label: label.to_owned(),
            sent,
            block,
            fee,
        });
        Ok(())
    }

    /// What the system plans read: the config, the accounting and the buffer's stablecoin
    /// holding (absent until the first credit).
    async fn system_source(&self, d: &Deployment) -> Res<Snapshot> {
        let buffer = d.holding_of(d.buffer().account_id, d.stable_def);
        self.snapshot(&[
            d.config.account_id,
            d.accounting.account_id,
            buffer.account_id,
        ])
        .await
    }

    /// The decoded state of `actor`, or an error naming `what`.
    async fn state<T>(&self, actor: Actor, what: &str, decode: fn(&[u8]) -> Option<T>) -> Res<T> {
        let bytes = self
            .raw_state(actor)
            .await?
            .ok_or_else(|| format!("no {what} state"))?;
        decode(&bytes).ok_or_else(|| format!("the {what} does not decode").into())
    }

    /// The balance of a per-definition holding (ADR-062); zero while it has no state.
    async fn balance(&self, holding: Actor) -> Res<u128> {
        Ok(match self.raw_state(holding).await? {
            None => 0,
            Some(bytes) => {
                Holding::decode(&bytes)
                    .ok_or("not a token holding")?
                    .balance
            }
        })
    }
}

/// The token, oracle and mock host the engine runs against.
struct Setup {
    p: Programs,
    params: Actor,
    params_key: Signer,
    oracle: Actor,
    whale: Signer,
    collateral_def: AccountId,
    stable_def: AccountId,
}

impl Setup {
    async fn deploy(live: &mut Live, p: Programs) -> Res<Self> {
        let params_key = live.signer("params");
        let whale = live.signer("whale");
        let cdef = live.signer("collateral-def");
        let sdef = live.signer("stable-def");
        let oracle_key = live.signer("oracle");
        let publisher = live.signer("publisher");
        let t = p.token;
        let s = Self {
            params: Actor::new(params_key.account, p.host),
            oracle: Actor::new(oracle_key.account, p.oracle),
            params_key,
            collateral_def: cdef.account,
            stable_def: sdef.account,
            whale,
            p,
        };
        let cdef_actor = Actor::new(cdef.account, t);
        let new_collateral = TokenMsg::NewDefinition {
            minter: None,
            initial_to: Some(s.whale.account),
            initial_supply: WHALE_COLLATERAL,
        };
        let evidence = live.key_evidence_if_fresh(&s.whale).await?;
        let actors = vec![cdef_actor, holding_actor(t, s.whale.account, cdef.account)];
        let pl = plan(
            cdef_actor,
            actors,
            vec![cdef.account],
            evidence,
            &new_collateral,
        )?;
        live.run("token: collateral def", &pl).await?;
        let sdef_actor = Actor::new(sdef.account, t);
        let new_stable = TokenMsg::NewDefinition {
            minter: Some(s.params),
            initial_to: None,
            initial_supply: 0,
        };
        let pl = plan(
            sdef_actor,
            vec![sdef_actor],
            vec![sdef.account],
            vec![],
            &new_stable,
        )?;
        live.run("token: stablecoin def", &pl).await?;
        let publisher_actor = Actor::new(publisher.account, s.p.relay);
        let init_oracle = OracleMsg::Init {
            publisher: publisher_actor,
        };
        let pl = plan(
            s.oracle,
            vec![s.oracle],
            vec![oracle_key.account],
            vec![],
            &init_oracle,
        )?;
        live.run("oracle: init", &pl).await?;
        // The redemption price: the lot-3 position is unsafe at it and the feed is in band.
        let publish = OracleMsg::Publish {
            price_ray: PRICE_RAY,
            timestamp_ms: live.now_ms().await?,
            confidence: 0,
        };
        let forward = Forward {
            to: s.oracle,
            message: borsh::to_vec(&publish)?,
        };
        let evidence = live.key_evidence_if_fresh(&publisher).await?;
        let actors = vec![publisher_actor, s.oracle];
        let pl = plan(publisher_actor, actors, vec![], evidence, &forward)?;
        live.run("oracle: price 0.5", &pl).await?;
        let init_host = HostMsg::Init {
            admin: s.params,
            token: t,
            stable_def: s.stable_def,
            collateral_def: s.collateral_def,
            oracle: s.oracle,
            redemption_price_ray: PRICE_RAY,
            min_cr_ray: MIN_CR_RAY,
        };
        let signers = vec![s.params_key.account];
        let pl = plan(s.params, vec![s.params], signers, vec![], &init_host)?;
        live.run("host: init", &pl).await?;
        Ok(s)
    }

    fn proxied(&self, message: HostMsg) -> HostMsg {
        HostMsg::Proxy {
            params: self.params.account_id,
            message: Box::new(message),
        }
    }

    /// What the mock host touches when it seizes `owner`'s position: the position PDA, the
    /// owner's host vault token actor and that vault's collateral holding (ADR-062).
    fn seized_actors(&self, owner: AccountId) -> [Actor; 3] {
        let (h, t) = (self.p.host, self.p.token);
        let vault = mock_host_core::vault_id(h, owner);
        [
            Actor::new(mock_host_core::position_id(h, owner), h),
            Actor::new(vault, t),
            holding_actor(t, vault, self.collateral_def),
        ]
    }

    /// `owner` gets `collateral` from the whale, opens a mock-host position with it and generates
    /// `debt` (minted to its own stablecoin holding). Returns the position account.
    async fn open_position(
        &self,
        live: &mut Live,
        owner: &Signer,
        collateral: u128,
        debt: u128,
    ) -> Res<AccountId> {
        let (t, cdef) = (self.p.token, self.collateral_def);
        let owner_collateral = holding_actor(t, owner.account, cdef);
        let whale_actor = Actor::new(self.whale.account, t);
        let fund = TokenMsg::Transfer {
            definition: cdef,
            to: owner.account,
            amount: collateral,
        };
        let evidence = live.key_evidence_if_fresh(owner).await?;
        let whale_collateral = holding_actor(t, self.whale.account, cdef);
        let actors = vec![whale_actor, whale_collateral, owner_collateral];
        let pl = plan(
            whale_actor,
            actors,
            vec![self.whale.account],
            evidence,
            &fund,
        )?;
        live.run("token: fund owner", &pl).await?;

        let proxy = Actor::new(owner.account, self.p.host);
        let [position, _, vault_collateral] = self.seized_actors(owner.account);
        let open = self.proxied(HostMsg::OpenPosition {
            owner: owner.account,
            collateral,
        });
        let actors = vec![
            proxy,
            self.params,
            position,
            Actor::new(owner.account, t),
            owner_collateral,
            vault_collateral,
        ];
        let pl = plan(proxy, actors, vec![owner.account], vec![], &open)?;
        live.run("host: open position", &pl).await?;

        let generate = self.proxied(HostMsg::GenerateDebt {
            normalized_debt: debt,
        });
        let owner_stable = holding_actor(t, owner.account, self.stable_def);
        let sdef = Actor::new(self.stable_def, t);
        let actors = vec![proxy, self.params, position, sdef, owner_stable];
        let pl = plan(proxy, actors, vec![owner.account], vec![], &generate)?;
        live.run("host: generate debt", &pl).await?;
        Ok(position.account_id)
    }

    /// `donor` transfers `amount` of stablecoin to the buffer's holding (credits are
    /// permissionless; a fresh holding self-admits through the token's `HOLDING` seed).
    async fn donate(
        &self,
        live: &mut Live,
        d: &Deployment,
        donor: &Signer,
        amount: u128,
    ) -> Res<()> {
        let t = self.p.token;
        let buffer = d.buffer().account_id;
        let from = Actor::new(donor.account, t);
        let actors = vec![
            from,
            holding_actor(t, donor.account, self.stable_def),
            d.holding_of(buffer, self.stable_def),
        ];
        let transfer = TokenMsg::Transfer {
            definition: self.stable_def,
            to: buffer,
            amount,
        };
        let pl = plan(from, actors, vec![donor.account], vec![], &transfer)?;
        live.run(&format!("token: donate {amount}"), &pl).await
    }
}

/// `Init` with the protocol token enabled and [`GENESIS`] to `taker`, `SetCollateral` and the
/// host bound to the config actor.
async fn engine_up(live: &mut Live, s: &Setup, taker: AccountId, slack_ms: u64) -> Res<Deployment> {
    let admin = live.signer("admin");
    let args = InitArgs {
        salt: SALT,
        params: EngineParamsArgs {
            protocol_token_enabled: true,
            ..EngineParamsArgs::defaults_for_tests()
        },
        host: s.params,
        token: s.p.token,
        stable_def: s.stable_def,
        oracle: s.oracle,
        clock: Actor::new(CLOCK_01_PROGRAM_ACCOUNT_ID, clock_core::clock_account_id()),
        slack_ms,
        ref_oracle: None,
        protocol_genesis: Some(ProtocolGenesis {
            to: taker,
            supply: GENESIS,
        }),
    };
    let init = sdk::plan_init(s.p.engine, admin.account, args)?;
    live.run("engine: Init", &init).await?;

    let config = engine_core::actors::config_actor(s.p.engine, admin.account, SALT);
    let snapshot = live.snapshot(&[config.account_id]).await?;
    let d = Deployment::load(&snapshot, s.p.engine, config.account_id)?;
    let now = live.now_ms().await?;
    let collateral = CollateralArgs::defaults_for_tests();
    let set = sdk::plan_set_collateral(&d, s.collateral_def, collateral, now)?;
    live.run("engine: SetCollateral", &set).await?;

    let bind = HostMsg::SetLiquidationEngine {
        engine_actor: d.config,
    };
    let pl = plan(
        s.params,
        vec![s.params],
        vec![s.params_key.account],
        vec![],
        &bind,
    )?;
    live.run("host: bind engine", &pl).await?;
    Ok(d)
}

async fn accounting(live: &Live, d: &Deployment) -> Res<AccountingState> {
    live.state(d.accounting, "accounting", AccountingState::decode)
        .await
}

/// The lot-3 position liquidated and settled empty; returns the shortfall `U` booked.
async fn book_shortfall(live: &mut Live, s: &Setup, d: &Deployment) -> Res<u128> {
    let owner = live.signer("owner");
    let position = s
        .open_position(live, &owner, SHORTFALL_COLLATERAL, SHORTFALL_DEBT)
        .await?;
    let keeper = live.signer("keeper");
    let args = LiquidateArgs {
        keeper: keeper.account,
        collateral_def: s.collateral_def,
        position,
        nonce: 0,
        now_ms: live.now_ms().await?,
    };
    let liq = sdk::plan_liquidate(d, &args, &s.seized_actors(owner.account))?;
    live.run("engine: Liquidate (lot 3)", &liq).await?;
    let settle = sdk::plan_settle_empty(d, s.collateral_def, position, 0)?;
    live.run("engine: SettleEmpty", &settle).await?;
    let unbacked = accounting(live, d).await?.unbacked_debt;
    println!("shortfall booked: unbacked_debt {unbacked}");
    Ok(unbacked)
}

/// `StartDebt 0` on the absent buffer holding, then `taker` takes the whole lot.
async fn debt_auction(live: &mut Live, d: &Deployment, taker: AccountId) -> Res<()> {
    let source = live.system_source(d).await?;
    if sdk::buffer_evidence(&source, d).len() != 1 {
        return Err("the buffer holding exists before any credit".into());
    }
    println!("buffer holding absent: StartDebt carries its Pda evidence");
    let start = sdk::plan_start_debt(&source, d, 0, live.now_ms().await?)?;
    live.run("engine: StartDebt", &start).await?;
    let auction = live
        .state(d.debt(0), "debt auction", DebtAuctionState::decode)
        .await?;
    let take = TakeArgs {
        taker,
        nonce: 0,
        amount_coin: auction.lot_left,
        limit: auction.start_amount,
        now_ms: live.now_ms().await?,
    };
    live.run("engine: TakeDebt (whole)", &sdk::plan_take_debt(d, take)?)
        .await
}

/// `StartSurplus 0`, then `taker` takes the whole lot, paying at most its protocol balance.
async fn surplus_auction(live: &mut Live, d: &Deployment, taker: AccountId) -> Res<()> {
    let source = live.system_source(d).await?;
    let start = sdk::plan_start_surplus(&source, d, 0, live.now_ms().await?)?;
    live.run("engine: StartSurplus", &start).await?;
    let auction = live
        .state(d.surplus(0), "surplus auction", SurplusAuctionState::decode)
        .await?;
    let take = TakeArgs {
        taker,
        nonce: 0,
        amount_coin: auction.lot_left,
        limit: live.balance(d.holding_of(taker, d.protocol_def)).await?,
        now_ms: live.now_ms().await?,
    };
    live.run(
        "engine: TakeSurplus (whole)",
        &sdk::plan_take_surplus(d, take)?,
    )
    .await
}

/// The whole system scenario; asserts the end state.
async fn system(live: &mut Live, s: &Setup, d: &Deployment, taker: &Signer) -> Res<()> {
    let shortfall = book_shortfall(live, s, d).await?;
    let lot = EngineParamsArgs::defaults_for_tests().debt_lot;
    let settle_donation = shortfall
        .checked_sub(lot)
        .filter(|amount| *amount > 0 && amount.saturating_add(SURPLUS_DONATION) <= DONOR_FUNDS)
        .ok_or_else(|| format!("unexpected shortfall {shortfall}"))?;
    s.open_position(live, taker, lot, lot).await?;
    debt_auction(live, d, taker.account).await?;

    let donor = live.signer("donor");
    s.open_position(live, &donor, DONOR_FUNDS, DONOR_FUNDS)
        .await?;
    s.donate(live, d, &donor, settle_donation).await?;
    let source = live.system_source(d).await?;
    let settle = sdk::plan_settle_debt(&source, d, u128::MAX)?;
    live.run("engine: SettleDebt (max)", &settle).await?;
    let settled = accounting(live, d).await?;
    if settled.unbacked_debt != 0 {
        return Err(format!("unbacked {} after the settle", settled.unbacked_debt).into());
    }
    s.donate(live, d, &donor, SURPLUS_DONATION).await?;
    surplus_auction(live, d, taker.account).await?;

    let source = live.system_source(d).await?;
    let check = sdk::plan_check_solvency(&source, d, live.now_ms().await?)?;
    live.run("engine: CheckSolvency", &check).await?;
    check_end_state(live, s, d, taker.account).await
}

/// `unbacked_debt` 0, both auctions `Ended`, the host not frozen (`Healthy`), and the protocol
/// supply equal to genesis + minted − burned, all of it held by the taker.
async fn check_end_state(live: &Live, s: &Setup, d: &Deployment, taker: AccountId) -> Res<()> {
    let acc = accounting(live, d).await?;
    let debt = live
        .state(d.debt(0), "debt auction", DebtAuctionState::decode)
        .await?;
    let surplus = live
        .state(d.surplus(0), "surplus auction", SurplusAuctionState::decode)
        .await?;
    let host = live.state(s.params, "host params", Params::decode).await?;
    let definition = Actor::new(d.protocol_def, d.token);
    let supply = live
        .state(definition, "protocol definition", Definition::decode)
        .await?
        .supply;
    let held = live.balance(d.holding_of(taker, d.protocol_def)).await?;
    let expected = GENESIS
        .checked_add(acc.total_protocol_minted)
        .and_then(|sum| sum.checked_sub(acc.total_protocol_burned))
        .ok_or("protocol totals overflow")?;
    println!(
        "end: unbacked {}, debt auction {:?}, surplus auction {:?}, host frozen {}, protocol supply {supply} = genesis {GENESIS} + minted {} - burned {}, taker holds {held}",
        acc.unbacked_debt,
        debt.phase,
        surplus.phase,
        host.is_frozen,
        acc.total_protocol_minted,
        acc.total_protocol_burned,
    );
    let ok = acc.unbacked_debt == 0
        && debt.phase == SystemPhase::Ended
        && surplus.phase == SystemPhase::Ended
        && !host.is_frozen
        && !acc.host_frozen_by_engine
        && acc.total_protocol_minted > 0
        && acc.total_protocol_burned > 0
        && supply == expected
        && held == supply;
    if !ok {
        return Err("the end state does not match the scenario".into());
    }
    Ok(())
}

fn print_summary(rows: &[Row]) {
    println!();
    println!(
        "| Transaction | Hash | Block | Simulated cycles | gas_limit | max_fee | Fee charged |"
    );
    println!("|---|---|---|---|---|---|---|");
    for row in rows {
        println!(
            "| {} | `{}` | {} | {} | {} | {} | {} |",
            row.label,
            HashType(row.sent.hash),
            row.block,
            row.sent.cycles,
            row.sent.gas_limit,
            row.sent.max_fee,
            row.fee
        );
    }
    let total: u128 = rows.iter().map(|row| row.fee).fold(0, u128::saturating_add);
    println!("total fees {total} over {} transactions", rows.len());
}

#[tokio::main]
async fn main() -> Res<()> {
    let programs = Programs {
        token: env_account("TOKEN_PROGRAM")?,
        oracle: env_account("ORACLE_PROGRAM")?,
        host: env_account("HOST_PROGRAM")?,
        relay: env_account("RELAY_PROGRAM")?,
        engine: env_account("ENGINE_PROGRAM")?,
    };
    let slack_ms = match env::var("SLACK_MS") {
        Ok(raw) => raw.parse()?,
        Err(_) => DEFAULT_SLACK_MS,
    };
    let mut live = Live::connect().await?;
    println!(
        "payer {} balance {}, head clock {} ms, slack {slack_ms} ms",
        live.payer,
        live.client.get_account_balance(live.payer).await?,
        live.now_ms().await?
    );
    let setup = Setup::deploy(&mut live, programs).await?;
    let taker = live.signer("taker");
    let outcome = match engine_up(&mut live, &setup, taker.account, slack_ms).await {
        Ok(d) => system(&mut live, &setup, &d, &taker).await,
        Err(err) => Err(err),
    };
    print_summary(&live.rows);
    outcome?;
    println!("system complete");
    Ok(())
}
