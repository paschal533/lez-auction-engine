//! A live session for the examples that drive a standalone sequencer (`examples/slice.rs`,
//! `examples/system.rs`, `examples/tier2.rs`, run by `v04/scripts/e2e.sh`): the environment,
//! derived signer keys, the simulate-send-confirm loop with its fee record, and the token, oracle and mock-host fixtures
//! the engine runs against.
//!
//! Environment: `SEQUENCER_URL` (default `http://127.0.0.1:3040`); `TOKEN_PROGRAM`,
//! `ORACLE_PROGRAM`, `HOST_PROGRAM`, `RELAY_PROGRAM`, `ENGINE_PROGRAM` (the deployed program
//! accounts); `PAYER`, the fee payer whose key the wallet (`LEE_WALLET_HOME_DIR`) holds; optional
//! `SLACK_MS`. Every other actor signs with a key derived from the session tag and its label;
//! only the payer pays fees.

use std::{collections::BTreeMap, env, error::Error, str::FromStr, time::Duration};

use borsh::BorshSerialize;
use clock_core::{CLOCK_01_PROGRAM_ACCOUNT_ID, ClockAccountData};
use common::HashType;
use lee::{AccountId, Actor, PrivateKey, PublicAccountEvidence, PublicKey};
use mock_host_core::{HostMsg, position_id, vault_id};
use oracle_core::OracleMsg;
use sequencer_service_rpc::{RpcClient as _, SequencerClient, SequencerClientBuilder};
use sha2::{Digest as _, Sha256};
use token_core::{Holding, TokenMsg, holding_actor};

use crate::{Plan, Sent, Snapshot, StateSource, live::Wallet};

/// The examples' error type.
pub type Res<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// 1e27.
pub const RAY: u128 = 1_000_000_000_000_000_000_000_000_000;
/// The mock host's minimum collateral ratio, 150 %.
pub const MIN_CR_RAY: u128 = RAY * 3 / 2;
/// The mock host's redemption price, 0.5 (`World::bare_at(RAY / 2)`).
pub const REDEMPTION_PRICE_RAY: u128 = RAY / 2;
/// The collateral supply, all minted to the whale that funds position owners.
pub const WHALE_COLLATERAL: u128 = 1_000_000_000_000;
/// The block interval of `networks/local.json` is 15 s; a claim made at the head block must
/// still cover the clock of the block that includes it (ADR-058).
pub const DEFAULT_SLACK_MS: u64 = 60_000;
const POLL: Duration = Duration::from_secs(1);
const INCLUSION_TIMEOUT: Duration = Duration::from_secs(180);

/// The relay guest's root message: forward `message` to `to`.
#[derive(BorshSerialize)]
pub struct Forward {
    pub to: Actor,
    pub message: Vec<u8>,
}

/// The deployed programs.
pub struct Programs {
    pub token: AccountId,
    pub oracle: AccountId,
    pub host: AccountId,
    pub relay: AccountId,
    pub engine: AccountId,
}

impl Programs {
    /// The five program accounts from the environment.
    ///
    /// # Errors
    /// If a variable is unset or not an account id.
    pub fn from_env() -> Res<Self> {
        Ok(Self {
            token: env_account("TOKEN_PROGRAM")?,
            oracle: env_account("ORACLE_PROGRAM")?,
            host: env_account("HOST_PROGRAM")?,
            relay: env_account("RELAY_PROGRAM")?,
            engine: env_account("ENGINE_PROGRAM")?,
        })
    }
}

/// One signer: its key and account.
#[derive(Clone)]
pub struct Signer {
    pub key: PrivateKey,
    pub account: AccountId,
}

impl Signer {
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        PublicKey::new_from_private_key(&self.key)
    }
}

/// One sent transaction, as the summary table prints it.
pub struct Row {
    pub label: String,
    pub sent: Sent,
    pub block: u64,
    pub fee: u128,
}

/// The account in the environment variable `name` (a leading `Public/` is accepted).
///
/// # Errors
/// If `name` is unset or not an account id.
pub fn env_account(name: &str) -> Res<AccountId> {
    let raw = env::var(name).map_err(|_| format!("{name} is not set"))?;
    let raw = raw.trim().trim_start_matches("Public/");
    AccountId::from_str(raw).map_err(|err| format!("{name}={raw}: {err:?}").into())
}

/// `SLACK_MS` from the environment, else [`DEFAULT_SLACK_MS`].
///
/// # Errors
/// If `SLACK_MS` is set but not a number.
pub fn slack_ms_from_env() -> Res<u64> {
    match env::var("SLACK_MS") {
        Ok(raw) => Ok(raw.parse()?),
        Err(_) => Ok(DEFAULT_SLACK_MS),
    }
}

/// SHA-256 of `tag/label`, re-hashed until it is a valid secp256k1 scalar.
#[must_use]
pub fn derive_key(tag: &str, label: &str) -> PrivateKey {
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
///
/// # Errors
/// If `msg` does not encode.
pub fn plan(
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

/// The every-block clock actor the engine's claims are asserted against.
#[must_use]
pub fn clock() -> Actor {
    Actor::new(CLOCK_01_PROGRAM_ACCOUNT_ID, clock_core::clock_account_id())
}

/// Prints `rows` as a Markdown table, then the total fee.
pub fn print_summary(rows: &[Row]) {
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

/// The live session: sequencer, wallet, payer, signers and the transactions sent so far.
pub struct Live {
    pub client: SequencerClient,
    pub payer: AccountId,
    pub rows: Vec<Row>,
    wallet: Wallet,
    tag: String,
    keys: BTreeMap<AccountId, PrivateKey>,
}

impl Live {
    /// Connects to `SEQUENCER_URL` with the wallet's key for `PAYER`; signer keys derive from
    /// `tag`.
    ///
    /// # Errors
    /// If the environment, the client or the wallet fails, or the wallet lacks the payer's key.
    pub fn connect(tag: String) -> Res<Self> {
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
        Ok(Self {
            client,
            payer,
            rows: Vec::new(),
            wallet,
            tag,
            keys: BTreeMap::from([(payer, payer_key)]),
        })
    }

    /// The signer `label` (key derived from the session tag); it signs every later send.
    pub fn signer(&mut self, label: &str) -> Signer {
        let key = derive_key(&self.tag, label);
        let account = AccountId::from(&PublicKey::new_from_private_key(&key));
        self.keys.insert(account, key.clone());
        Signer { key, account }
    }

    /// `accounts` read from the sequencer now.
    ///
    /// # Errors
    /// If the sequencer fails.
    pub async fn snapshot(&self, accounts: &[AccountId]) -> Res<Snapshot> {
        Ok(Snapshot::fetch(&self.client, accounts.iter().copied()).await?)
    }

    /// Whether `account` exists.
    ///
    /// # Errors
    /// If the sequencer fails.
    pub async fn exists(&self, account: AccountId) -> Res<bool> {
        Ok(self.snapshot(&[account]).await?.account_exists(account))
    }

    /// The raw state of `actor`, `None` if absent.
    ///
    /// # Errors
    /// If the sequencer fails.
    pub async fn raw_state(&self, actor: Actor) -> Res<Option<Vec<u8>>> {
        Ok(self.snapshot(&[actor.account_id]).await?.actor_state(actor))
    }

    /// The decoded state of `actor`, or an error naming `what`.
    ///
    /// # Errors
    /// If the sequencer fails, or the state is absent or does not decode.
    pub async fn state<T>(
        &self,
        actor: Actor,
        what: &str,
        decode: fn(&[u8]) -> Option<T>,
    ) -> Res<T> {
        let bytes = self
            .raw_state(actor)
            .await?
            .ok_or_else(|| format!("no {what} state"))?;
        decode(&bytes).ok_or_else(|| format!("the {what} does not decode").into())
    }

    /// The balance of a per-definition holding (ADR-062); zero while it has no state.
    ///
    /// # Errors
    /// If the sequencer fails or the state is not a holding.
    pub async fn balance(&self, holding: Actor) -> Res<u128> {
        Ok(match self.raw_state(holding).await? {
            None => 0,
            Some(bytes) => {
                Holding::decode(&bytes)
                    .ok_or("not a token holding")?
                    .balance
            }
        })
    }

    /// `Key` evidence for `signer`'s account while it does not exist (a fresh non-signer
    /// holding owner, ruling R-3a).
    ///
    /// # Errors
    /// If the sequencer fails.
    pub async fn key_evidence_if_fresh(&self, signer: &Signer) -> Res<Vec<PublicAccountEvidence>> {
        Ok(if self.exists(signer.account).await? {
            Vec::new()
        } else {
            vec![PublicAccountEvidence::Key(signer.public_key())]
        })
    }

    /// The every-block clock's timestamp at the head, ms: the time claims are made at.
    ///
    /// # Errors
    /// If the sequencer fails or the clock holds no clock state.
    pub async fn now_ms(&self) -> Res<u64> {
        let bytes = self
            .raw_state(clock())
            .await?
            .ok_or("the clock account holds no clock state: wrong clock program id?")?;
        Ok(ClockAccountData::from_bytes(&bytes).timestamp)
    }

    /// Simulates, fits, sends and confirms `plan`, then records the fee the payer was charged.
    ///
    /// # Errors
    /// If any step fails, or the fee charged exceeds the declared `max_fee`.
    pub async fn run(&mut self, label: &str, plan: &Plan) -> Res<()> {
        let before = self.client.get_account_balance(self.payer).await?;
        let keys: Vec<&PrivateKey> = self.keys.values().collect();
        let sent = crate::send_signed(&self.client, &self.wallet, plan, self.payer, &keys)
            .await
            .map_err(|err| format!("{label}: {err}"))?;
        let block = crate::confirm(&self.client, sent.hash, POLL, INCLUSION_TIMEOUT)
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
}

/// The token, oracle and mock host the engine runs against (`World::bare_at(RAY / 2)`).
pub struct Setup {
    pub p: Programs,
    /// The host params actor; `params_key` signs its admin messages.
    pub params: Actor,
    pub params_key: Signer,
    pub oracle: Actor,
    /// The oracle's publisher, a relay actor at this signer's account.
    pub publisher: Signer,
    pub whale: Signer,
    pub collateral_def: AccountId,
    pub stable_def: AccountId,
}

impl Setup {
    /// Deploys the collateral and stablecoin definitions, the oracle (first publication
    /// `first_price` = `(label, price_ray)`) and the mock host at [`REDEMPTION_PRICE_RAY`].
    ///
    /// # Errors
    /// If a send fails.
    pub async fn deploy(live: &mut Live, p: Programs, first_price: (&str, u128)) -> Res<Self> {
        let params_key = live.signer("params");
        let cdef = live.signer("collateral-def");
        let sdef = live.signer("stable-def");
        let oracle_key = live.signer("oracle");
        let s = Self {
            params: Actor::new(params_key.account, p.host),
            oracle: Actor::new(oracle_key.account, p.oracle),
            params_key,
            publisher: live.signer("publisher"),
            whale: live.signer("whale"),
            collateral_def: cdef.account,
            stable_def: sdef.account,
            p,
        };
        s.define_tokens(live, &cdef, &sdef).await?;
        let init_oracle = OracleMsg::Init {
            publisher: Actor::new(s.publisher.account, s.p.relay),
        };
        let signers = vec![oracle_key.account];
        let pl = plan(s.oracle, vec![s.oracle], signers, vec![], &init_oracle)?;
        live.run("oracle: init", &pl).await?;
        let (label, price_ray) = first_price;
        s.publish(live, label, price_ray).await?;
        let init_host = HostMsg::Init {
            admin: s.params,
            token: s.p.token,
            stable_def: s.stable_def,
            collateral_def: s.collateral_def,
            oracle: s.oracle,
            redemption_price_ray: REDEMPTION_PRICE_RAY,
            min_cr_ray: MIN_CR_RAY,
        };
        let signers = vec![s.params_key.account];
        let pl = plan(s.params, vec![s.params], signers, vec![], &init_host)?;
        live.run("host: init", &pl).await?;
        Ok(s)
    }

    /// The collateral definition (whole supply to the whale) and the stablecoin definition
    /// (minter: the host params actor).
    async fn define_tokens(&self, live: &mut Live, cdef: &Signer, sdef: &Signer) -> Res<()> {
        let t = self.p.token;
        let cdef_actor = Actor::new(cdef.account, t);
        let new_collateral = TokenMsg::NewDefinition {
            minter: None,
            initial_to: Some(self.whale.account),
            initial_supply: WHALE_COLLATERAL,
        };
        let evidence = live.key_evidence_if_fresh(&self.whale).await?;
        let actors = vec![
            cdef_actor,
            holding_actor(t, self.whale.account, cdef.account),
        ];
        let signers = vec![cdef.account];
        let pl = plan(cdef_actor, actors, signers, evidence, &new_collateral)?;
        live.run("token: collateral def", &pl).await?;
        let sdef_actor = Actor::new(sdef.account, t);
        let new_stable = TokenMsg::NewDefinition {
            minter: Some(self.params),
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
        live.run("token: stablecoin def", &pl).await
    }

    /// The publisher (a relay actor) publishes `price_ray` at the head clock's time, with zero
    /// confidence.
    ///
    /// # Errors
    /// If the send fails.
    pub async fn publish(&self, live: &mut Live, label: &str, price_ray: u128) -> Res<()> {
        self.publish_with_confidence(live, label, price_ray, 0)
            .await
    }

    /// [`Setup::publish`] with `confidence` (raw, in the oracle's scale: RAY for the e2e types).
    ///
    /// # Errors
    /// If the send fails.
    pub async fn publish_with_confidence(
        &self,
        live: &mut Live,
        label: &str,
        price_ray: u128,
        confidence: u128,
    ) -> Res<()> {
        let publisher = Actor::new(self.publisher.account, self.p.relay);
        let publish = OracleMsg::Publish {
            price_ray,
            timestamp_ms: live.now_ms().await?,
            confidence,
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

    /// Binds the host to the engine's config actor `config`.
    ///
    /// # Errors
    /// If the send fails.
    pub async fn bind(&self, live: &mut Live, config: Actor) -> Res<()> {
        let bind = HostMsg::SetLiquidationEngine {
            engine_actor: config,
        };
        let signers = vec![self.params_key.account];
        let pl = plan(self.params, vec![self.params], signers, vec![], &bind)?;
        live.run("host: bind engine", &pl).await
    }

    fn proxied(&self, message: HostMsg) -> HostMsg {
        HostMsg::Proxy {
            params: self.params.account_id,
            message: Box::new(message),
        }
    }

    /// What the mock host touches when it seizes `owner`'s position: the position PDA, the
    /// owner's host vault token actor and that vault's collateral holding (ADR-062). The SDK
    /// takes these from the caller, since they are host-specific (`plan_liquidate`).
    #[must_use]
    pub fn seized_actors(&self, owner: AccountId) -> [Actor; 3] {
        let (h, t) = (self.p.host, self.p.token);
        let vault = vault_id(h, owner);
        [
            Actor::new(position_id(h, owner), h),
            Actor::new(vault, t),
            holding_actor(t, vault, self.collateral_def),
        ]
    }

    /// `owner` gets `collateral` from the whale, opens a mock-host position with it and generates
    /// `debt` (minted to its own stablecoin holding). Returns the position account.
    ///
    /// # Errors
    /// If a send fails.
    pub async fn open_position(
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
        let signers = vec![self.whale.account];
        let pl = plan(whale_actor, actors, signers, evidence, &fund)?;
        live.run("token: fund owner", &pl).await?;

        let proxy = Actor::new(owner.account, self.p.host);
        let [position, _, vault_collateral] = self.seized_actors(owner.account);
        let open = self.proxied(HostMsg::OpenPosition {
            owner: owner.account,
            collateral,
        });
        let owner_actor = Actor::new(owner.account, t);
        let actors = vec![
            proxy,
            self.params,
            position,
            owner_actor,
            owner_collateral,
            vault_collateral,
        ];
        let pl = plan(proxy, actors, vec![owner.account], vec![], &open)?;
        live.run("host: open position", &pl).await?;

        let generate = self.proxied(HostMsg::GenerateDebt {
            normalized_debt: debt,
        });
        let sdef = Actor::new(self.stable_def, t);
        let owner_stable = holding_actor(t, owner.account, self.stable_def);
        let actors = vec![proxy, self.params, position, sdef, owner_stable];
        let pl = plan(proxy, actors, vec![owner.account], vec![], &generate)?;
        live.run("host: generate debt", &pl).await?;
        Ok(position.account_id)
    }
}
