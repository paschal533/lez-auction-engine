//! The liquidation slice end to end on a live LEZ v0.4.0-rc1 sequencer, through the SDK:
//! `init → set collateral → open an unsafe position (mock host) → liquidate → partial bid →
//! final bid → claim leftover`, printing each transaction's hash, simulated cycles, declared
//! `gas_limit` and the fee charged (the payer's balance before and after), and asserting the end
//! state (auction `Settled`, auction vault empty after the claim).
//!
//! Driven by `v04/scripts/e2e.sh`, which deploys the guests and sets:
//! - `SEQUENCER_URL` (default `http://127.0.0.1:3040`);
//! - `TOKEN_PROGRAM`, `ORACLE_PROGRAM`, `HOST_PROGRAM`, `RELAY_PROGRAM`, `ENGINE_PROGRAM`: the
//!   deployed program accounts (the loader's header accounts);
//! - `PAYER`: the fee payer, whose key the wallet (`LEE_WALLET_HOME_DIR`) holds;
//! - optional `RUN_TAG` (fresh actor keys per tag) and `SLACK_MS` (the config's clock slack).
//!
//! Every other actor (admin, host params, token definitions, owners, keeper, bidder) signs with a
//! key derived from `RUN_TAG` and its label; only the payer pays fees.

use std::{collections::BTreeMap, env, error::Error, str::FromStr, time::Duration};

use auction_engine_params::args::{CollateralArgs, EngineParamsArgs};
use borsh::BorshSerialize;
use clock_core::{CLOCK_01_PROGRAM_ACCOUNT_ID, ClockAccountData};
use common::HashType;
use engine_core::{
    actors::record_actor,
    state::{AuctionPhase, AuctionState, LiquidationRecordState},
};
use lee::{AccountId, Actor, PrivateKey, PublicAccountEvidence, PublicKey};
use mock_host_core::{HostMsg, is_unsafe, vault_seed};
use oracle_core::OracleMsg;
use sdk::{
    BidArgs, Deployment, InitArgs, LiquidateArgs, Plan, Sent, Snapshot, StateSource, live::Wallet,
};
use sequencer_service_rpc::{RpcClient as _, SequencerClient, SequencerClientBuilder};
use sha2::{Digest as _, Sha256};
use token_core::{Holding, TokenMsg};

type Res<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// 1e27.
const RAY: u128 = 1_000_000_000_000_000_000_000_000_000;
/// The mock host's minimum collateral ratio, 150 %.
const MIN_CR_RAY: u128 = RAY * 3 / 2;
/// The oracle price the run starts at.
const SAFE_PRICE_RAY: u128 = 2 * RAY;
const SALT: u64 = 1;
const WHALE_COLLATERAL: u128 = 1_000_000_000_000;
/// The scenario of `harness/tests/cycles.rs` (CYCLES.md): collateral 500_000, debt 600_000,
/// redemption price and oracle 0.5.
const COLLATERAL: u128 = 500_000;
const DEBT: u128 = 600_000;
const PARTIAL: u128 = 100_000;
const TARGET: u128 = 660_000;
const FUNDS: u128 = 1_000_000;
/// The block interval of `networks/local.json` is 15 s; a claim made at the head block must
/// still cover the clock of the block that includes it.
const DEFAULT_SLACK_MS: u64 = 60_000;
const POLL: Duration = Duration::from_secs(1);
const INCLUSION_TIMEOUT: Duration = Duration::from_secs(180);

/// The relay guest's root message: forward `message` to `to`.
#[derive(BorshSerialize)]
struct Forward {
    to: Actor,
    message: Vec<u8>,
}

/// The mock host's side of a liquidation (Task 12 review: `plan_liquidate` takes the host's
/// actors from the caller, since they are host-specific).
mod mock_host {
    use lee::{AccountId, Actor};
    use mock_host_core::{position_id, vault_id};

    /// What the mock host touches when it seizes `owner`'s position: the position PDA and the
    /// owner's host vault holding.
    pub fn seized_actors(host: AccountId, token: AccountId, owner: AccountId) -> [Actor; 2] {
        [
            Actor::new(position_id(host, owner), host),
            Actor::new(vault_id(host, owner), token),
        ]
    }
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

impl Signer {
    fn public_key(&self) -> PublicKey {
        PublicKey::new_from_private_key(&self.key)
    }
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
    wallet: Wallet,
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

/// A plan for a non-engine message (token, oracle, host, relay): the SDK simulates, fits and
/// sends any program's message the same way.
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
        let wallet = Wallet::from_env_with_client(client.clone())?;
        let payer = env_account("PAYER")?;
        let payer_key = wallet
            .storage()
            .key_chain()
            .pub_account_signing_key(payer)
            .cloned()
            .ok_or_else(|| format!("the wallet holds no key for the payer {payer}"))?;
        let tag = env::var("RUN_TAG").unwrap_or_else(|_| "slice".into());
        let keys = BTreeMap::from([(payer, payer_key)]);
        Ok(Self {
            client,
            wallet,
            payer,
            tag,
            keys,
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

    async fn exists(&self, account: AccountId) -> Res<bool> {
        Ok(self.snapshot(&[account]).await?.account_exists(account))
    }

    async fn raw_state(&self, actor: Actor) -> Res<Option<Vec<u8>>> {
        Ok(self.snapshot(&[actor.account_id]).await?.actor_state(actor))
    }

    /// `Key` evidence for `signer`'s account while it does not exist (a fresh non-signer
    /// holding, ruling R-3a).
    async fn key_evidence_if_fresh(&self, signer: &Signer) -> Res<Vec<PublicAccountEvidence>> {
        Ok(if self.exists(signer.account).await? {
            Vec::new()
        } else {
            vec![PublicAccountEvidence::Key(signer.public_key())]
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
            "{label:<24} tx {} block {block:>4} cycles {:>9} gas_limit {:>9} max_fee {:>9} fee {fee:>9}",
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
}

/// The token, oracle and mock host the engine runs against (`World::bare_at(RAY / 2)`).
struct Setup {
    p: Programs,
    params: Actor,
    oracle: Actor,
    publisher: Signer,
    whale: Signer,
    collateral_def: AccountId,
    stable_def: AccountId,
}

impl Setup {
    async fn deploy(live: &mut Live, p: Programs) -> Res<Self> {
        let params = live.signer("params");
        let whale = live.signer("whale");
        let cdef = live.signer("collateral-def");
        let sdef = live.signer("stable-def");
        let oracle_key = live.signer("oracle");
        let publisher = live.signer("publisher");
        let s = Self {
            params: Actor::new(params.account, p.host),
            oracle: Actor::new(oracle_key.account, p.oracle),
            publisher,
            collateral_def: cdef.account,
            stable_def: sdef.account,
            whale,
            p,
        };
        let (t, cdef_actor) = (s.p.token, Actor::new(cdef.account, s.p.token));
        let whale_holding = Actor::new(s.whale.account, t);
        let new_collateral = TokenMsg::NewDefinition {
            minter: None,
            initial_to: Some(s.whale.account),
            initial_supply: WHALE_COLLATERAL,
        };
        let evidence = live.key_evidence_if_fresh(&s.whale).await?;
        let actors = vec![cdef_actor, whale_holding];
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
        let init_oracle = OracleMsg::Init {
            publisher: Actor::new(s.publisher.account, s.p.relay),
        };
        let pl = plan(
            s.oracle,
            vec![s.oracle],
            vec![oracle_key.account],
            vec![],
            &init_oracle,
        )?;
        live.run("oracle: init", &pl).await?;
        s.publish(live, "oracle: safe price", SAFE_PRICE_RAY)
            .await?;
        let init_host = HostMsg::Init {
            admin: s.params,
            token: t,
            stable_def: s.stable_def,
            collateral_def: s.collateral_def,
            oracle: s.oracle,
            redemption_price_ray: RAY / 2,
            min_cr_ray: MIN_CR_RAY,
        };
        let pl = plan(
            s.params,
            vec![s.params],
            vec![params.account],
            vec![],
            &init_host,
        )?;
        live.run("host: init", &pl).await?;
        Ok(s)
    }

    /// The publisher (a relay actor) publishes `price_ray` at the head clock's time.
    async fn publish(&self, live: &mut Live, label: &str, price_ray: u128) -> Res<()> {
        let publisher = Actor::new(self.publisher.account, self.p.relay);
        let publish = OracleMsg::Publish {
            price_ray,
            timestamp_ms: live.now_ms().await?,
            confidence: 0,
        };
        let forward = Forward {
            to: self.oracle,
            message: borsh::to_vec(&publish)?,
        };
        let evidence = live.key_evidence_if_fresh(&self.publisher).await?;
        let actors = vec![publisher, self.oracle];
        let pl = plan(publisher, actors, vec![], evidence, &forward)?;
        live.run(label, &pl).await
    }

    fn proxied(&self, message: HostMsg) -> HostMsg {
        HostMsg::Proxy {
            params: self.params.account_id,
            message: Box::new(message),
        }
    }

    /// `owner` gets `collateral` from the whale, opens a mock-host position with it and generates
    /// `debt` (minted to its own holding). Returns the position account.
    async fn open_position(
        &self,
        live: &mut Live,
        owner: &Signer,
        collateral: u128,
        debt: u128,
    ) -> Res<AccountId> {
        let (t, h) = (self.p.token, self.p.host);
        let owner_holding = Actor::new(owner.account, t);
        let whale_holding = Actor::new(self.whale.account, t);
        let fund = TokenMsg::Transfer {
            definition: self.collateral_def,
            to: owner.account,
            amount: collateral,
        };
        let evidence = live.key_evidence_if_fresh(owner).await?;
        let actors = vec![whale_holding, owner_holding];
        let pl = plan(
            whale_holding,
            actors,
            vec![self.whale.account],
            evidence,
            &fund,
        )?;
        live.run("token: fund owner", &pl).await?;

        let proxy = Actor::new(owner.account, h);
        let [position, vault] = mock_host::seized_actors(h, t, owner.account);
        let open = self.proxied(HostMsg::OpenPosition {
            owner: owner.account,
            collateral,
        });
        let evidence = vec![PublicAccountEvidence::Pda {
            program: h,
            seed: vault_seed(owner.account),
        }];
        let actors = vec![proxy, self.params, position, owner_holding, vault];
        let pl = plan(proxy, actors, vec![owner.account], evidence, &open)?;
        live.run("host: open position", &pl).await?;

        let generate = self.proxied(HostMsg::GenerateDebt {
            normalized_debt: debt,
        });
        let sdef = Actor::new(self.stable_def, t);
        let actors = vec![proxy, self.params, position, sdef, owner_holding];
        let pl = plan(proxy, actors, vec![owner.account], vec![], &generate)?;
        live.run("host: generate debt", &pl).await?;
        Ok(position.account_id)
    }

    /// Half the price at which the position would sit exactly at the minimum ratio.
    fn unsafe_price(collateral: u128, debt: u128) -> Res<u128> {
        let price = debt
            .checked_mul(MIN_CR_RAY)
            .and_then(|needed| needed.checked_div(collateral))
            .and_then(|at_ratio| at_ratio.checked_div(2))
            .ok_or("unsafe price overflows")?
            .max(1);
        if !is_unsafe(collateral, debt, price, MIN_CR_RAY) {
            return Err("the position is not unsafe at the published price".into());
        }
        Ok(price)
    }
}

/// `Init`, `SetCollateral` and the host bound to the config actor.
async fn engine_up(live: &mut Live, s: &Setup, slack_ms: u64) -> Res<Deployment> {
    let admin = live.signer("admin");
    let args = InitArgs {
        salt: SALT,
        params: EngineParamsArgs::defaults_for_tests(),
        host: s.params,
        token: s.p.token,
        stable_def: s.stable_def,
        oracle: s.oracle,
        clock: Actor::new(CLOCK_01_PROGRAM_ACCOUNT_ID, clock_core::clock_account_id()),
        slack_ms,
    };
    let init = sdk::plan_init(s.p.engine, admin.account, args)?;
    live.run("engine: Init", &init).await?;

    let config = engine_core::actors::config_actor(s.p.engine, admin.account, SALT);
    let snapshot = live.snapshot(&[config.account_id]).await?;
    let d = Deployment::load(&snapshot, s.p.engine, config.account_id)?;
    let now = live.now_ms().await?;
    let set = sdk::plan_set_collateral(
        &d,
        s.collateral_def,
        CollateralArgs::defaults_for_tests(),
        now,
    )?;
    live.run("engine: SetCollateral", &set).await?;

    let bind = HostMsg::SetLiquidationEngine {
        engine_actor: d.config,
    };
    let params_key = live.signer("params");
    let pl = plan(
        s.params,
        vec![s.params],
        vec![params_key.account],
        vec![],
        &bind,
    )?;
    live.run("host: bind engine", &pl).await?;
    Ok(d)
}

/// The record's auction count for `position`: the nonce the next liquidation must name.
async fn next_nonce(live: &Live, d: &Deployment, position: AccountId) -> Res<u64> {
    let record = record_actor(d.engine, d.config.account_id, position);
    Ok(match live.raw_state(record).await? {
        None => 0,
        Some(bytes) => {
            LiquidationRecordState::decode(&bytes)
                .ok_or("the record does not decode")?
                .count
        }
    })
}

async fn auction_state(
    live: &Live,
    d: &Deployment,
    position: AccountId,
    nonce: u64,
) -> Res<AuctionState> {
    let bytes = live
        .raw_state(d.auction(position, nonce))
        .await?
        .ok_or("no auction state")?;
    Ok(AuctionState::decode(&bytes).ok_or("the auction does not decode")?)
}

async fn balance(live: &Live, holding: Actor, definition: AccountId) -> Res<u128> {
    Ok(match live.raw_state(holding).await? {
        None => 0,
        Some(bytes) => Holding::decode(&bytes)
            .ok_or("not a token holding")?
            .balance(definition),
    })
}

/// Liquidate, two bids and the leftover claim; asserts the end state.
async fn auction(live: &mut Live, s: &Setup, d: &Deployment) -> Res<()> {
    let owner = live.signer("owner");
    let position = s.open_position(live, &owner, COLLATERAL, DEBT).await?;
    let unsafe_price = Setup::unsafe_price(COLLATERAL, DEBT)?;
    s.publish(live, "oracle: unsafe price", unsafe_price)
        .await?;
    s.publish(live, "oracle: price 0.5", RAY / 2).await?;

    let keeper = live.signer("keeper");
    let nonce = next_nonce(live, d, position).await?;
    let args = LiquidateArgs {
        keeper: keeper.account,
        collateral_def: s.collateral_def,
        position,
        nonce,
        now_ms: live.now_ms().await?,
    };
    let host_actors = mock_host::seized_actors(s.p.host, s.p.token, owner.account);
    let liq = sdk::plan_liquidate(d, &args, &host_actors)?;
    live.run("engine: Liquidate", &liq).await?;

    let bidder = live.signer("bidder");
    s.open_position(live, &bidder, FUNDS, FUNDS).await?;
    for (label, max_debt_in) in [
        ("engine: Bid (partial)", PARTIAL),
        ("engine: Bid (final)", TARGET),
    ] {
        let args = BidArgs {
            bidder: bidder.account,
            collateral_def: s.collateral_def,
            position,
            nonce,
            max_debt_in,
            min_collateral_out: 0,
            now_ms: live.now_ms().await?,
        };
        let buffer = live.snapshot(&[d.buffer().account_id]).await?;
        let bid = sdk::plan_bid(&buffer, d, &args)?;
        live.run(label, &bid).await?;
    }
    let settled = auction_state(live, d, position, nonce).await?;
    if settled.phase != AuctionPhase::Settled {
        return Err(format!("auction phase {:?}, expected Settled", settled.phase).into());
    }
    if settled.leftover_claimable == 0 {
        return Err("no leftover to claim".into());
    }

    let claim = sdk::plan_claim_leftover(d, owner.account, position, nonce)?;
    live.run("engine: ClaimLeftover", &claim).await?;
    let vault = balance(live, d.vault(position, nonce), s.collateral_def).await?;
    let owner_got = balance(live, d.holding(owner.account), s.collateral_def).await?;
    let claimed = auction_state(live, d, position, nonce).await?;
    println!(
        "auction: Settled, debt_raised {}, penalty_raised {}, collateral_sold {}, leftover {} claimed by the owner; vault {vault}",
        claimed.debt_raised,
        claimed.penalty_raised,
        claimed.collateral_sold,
        claimed.leftover_claimed
    );
    if vault != 0
        || owner_got != settled.leftover_claimable
        || claimed.leftover_claimed != settled.leftover_claimable
    {
        return Err(format!(
            "after the claim: vault {vault}, owner {owner_got}, claimed {} of {}",
            claimed.leftover_claimed, settled.leftover_claimable
        )
        .into());
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
        "payer {} balance {}",
        live.payer,
        live.client.get_account_balance(live.payer).await?
    );
    println!(
        "head clock {} ms, slack {slack_ms} ms",
        live.now_ms().await?
    );
    let setup = Setup::deploy(&mut live, programs).await?;
    let d = engine_up(&mut live, &setup, slack_ms).await?;
    let outcome = auction(&mut live, &setup, &d).await;
    print_summary(&live.rows);
    outcome?;
    println!("slice complete");
    Ok(())
}
