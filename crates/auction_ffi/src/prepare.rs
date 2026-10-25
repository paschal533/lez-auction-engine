//! `prepare`: builds the plans (`Plan` as JSON) the module signs and sends through `lez_core`.
//! This crate never signs. Each action is one `auction_sdk::ops::prepare_*` (the function the CLI
//! prints), run at the chain's `CLOCK_01` on a verified config (I4); the reply is its steps, facts
//! and touched accounts. Account arguments are base58 ids or wallet labels; u128 values are decimal
//! strings.

use auction_sdk::{
    chain::clock_ms,
    demo,
    deployment::program_id_hex,
    keystore::Keystore,
    ops::{
        self, Accounts, BidRequest, CallerRequest, ClaimLeftoverRequest, EngineParamsSlots,
        LiquidateRequest, OraclePublishRequest, Prepared, SettleDebtRequest, SettleEmptyRequest,
        TakeRequest, TokenTransferRequest, Touch,
    },
    verify::VerifiedConfig,
    Chain, Plan, SdkError, SdkResult,
};
use lee_core::account::AccountId;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    ctx::Ctx,
    envelope::{parse, FfiError},
    quotes::{default_slippage, slippage},
};

/// The CLI's default `--base-window-ms` of `dev oracle-publish`.
const DEFAULT_BASE_WINDOW_MS: u64 = 3_600_000;

fn default_base_window() -> u64 {
    DEFAULT_BASE_WINDOW_MS
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Action {
    Liquidate {
        position: String,
        keeper: String,
        keeper_holding: String,
    },
    Bid {
        auction: String,
        #[serde(with = "auction_sdk::dec")]
        coin_in_max: u128,
        bidder_stable: String,
        bidder_collateral: String,
        #[serde(default = "default_slippage")]
        slippage_bps: u32,
        #[serde(default, with = "auction_sdk::dec::opt")]
        min_out: Option<u128>,
    },
    ClaimLeftover {
        owner: String,
        owner_holding: String,
        auction: String,
    },
    SettleEmpty {
        caller: String,
        auction: String,
    },
    ObserveFeed {
        caller: String,
    },
    SettleDebt {
        caller: String,
        #[serde(default, with = "auction_sdk::dec::opt")]
        amount: Option<u128>,
    },
    CheckSolvency {
        caller: String,
    },
    StartSurplus {
        caller: String,
    },
    TakeSurplus(Take),
    CloseSurplus {
        caller: String,
    },
    StartDebt {
        caller: String,
    },
    TakeDebt(Take),
    CloseDebt {
        caller: String,
    },
    TokenTransfer {
        from: String,
        to: String,
        #[serde(with = "auction_sdk::dec")]
        amount: u128,
    },
    OraclePublish {
        price_source: String,
        #[serde(with = "auction_sdk::dec")]
        price: u128,
        host_admin: String,
        engine_admin: String,
        #[serde(default = "default_base_window")]
        base_window_ms: u64,
    },
    /// The demo's freeze trigger: `set_engine_params` with `demo::engine_params()` and the
    /// protocol token disabled (scripts/lib/demo-scenario.sh).
    DisableProtocolToken {
        admin: String,
    },
}

#[derive(Deserialize)]
struct Take {
    payer: String,
    receiver: String,
    #[serde(with = "auction_sdk::dec")]
    amount: u128,
    #[serde(default = "default_slippage")]
    slippage_bps: u32,
}

impl Take {
    fn request(&self) -> Result<TakeRequest, FfiError> {
        Ok(TakeRequest {
            payer: self.payer.clone(),
            receiver: self.receiver.clone(),
            amount: self.amount,
            slippage_bps: slippage(self.slippage_bps)?,
        })
    }
}

fn caller(c: &str) -> CallerRequest {
    CallerRequest {
        caller: c.to_owned(),
    }
}

/// The `prepare` op: `{"steps":[Plan,...],"facts":[{key,value}],"touched":[...]}`.
pub fn handle(ctx: &Ctx, req: &Value) -> Result<Value, FfiError> {
    let action: Action = parse(req)?;
    let resolve = |arg: &str| account(ctx, arg);
    let accounts = Accounts { resolve: &resolve };
    Ok(prepared_json(&run(ctx, &action, &accounts)?))
}

fn run(ctx: &Ctx, action: &Action, accounts: &Accounts<'_>) -> Result<Prepared, FfiError> {
    match action {
        // No verified config: a dev setup step on the deployment's token program.
        Action::TokenTransfer { from, to, amount } => {
            ctx.require_dev()?;
            let req = TokenTransferRequest {
                from: from.clone(),
                to: to.clone(),
                amount: *amount,
            };
            return Ok(ops::prepare_token_transfer(
                &ctx.deployment,
                &req,
                accounts,
            )?);
        }
        Action::OraclePublish { .. } => ctx.require_dev()?,
        _ => {}
    }
    let chain = ctx.chain.as_ref();
    let v = ctx.verified()?;
    let now = clock_ms(chain)?;
    engine_action(ctx, chain, &v, action, accounts, now)
}

fn engine_action(
    ctx: &Ctx,
    chain: &dyn Chain,
    v: &VerifiedConfig,
    action: &Action,
    accounts: &Accounts<'_>,
    now: u64,
) -> Result<Prepared, FfiError> {
    let p = match action {
        Action::Liquidate {
            position,
            keeper,
            keeper_holding,
        } => {
            let req = LiquidateRequest {
                position: position.clone(),
                keeper: keeper.clone(),
                keeper_holding: keeper_holding.clone(),
            };
            ops::prepare_liquidate(chain, v, &req, accounts, now)
        }
        Action::Bid {
            auction,
            coin_in_max,
            bidder_stable,
            bidder_collateral,
            slippage_bps,
            min_out,
        } => {
            let req = BidRequest {
                auction: auction.clone(),
                coin_in_max: *coin_in_max,
                bidder_stable: bidder_stable.clone(),
                bidder_collateral: bidder_collateral.clone(),
                slippage_bps: slippage(*slippage_bps)?,
                min_out: *min_out,
            };
            ops::prepare_bid(chain, v, &req, accounts, now)
        }
        Action::ClaimLeftover {
            owner,
            owner_holding,
            auction,
        } => {
            let req = ClaimLeftoverRequest {
                owner: owner.clone(),
                owner_holding: owner_holding.clone(),
                auction: auction.clone(),
            };
            ops::prepare_claim_leftover(chain, v, &req, accounts, now)
        }
        Action::SettleEmpty { caller, auction } => {
            let req = SettleEmptyRequest {
                caller: caller.clone(),
                auction: auction.clone(),
            };
            ops::prepare_settle_empty(chain, v, &req, accounts, now)
        }
        Action::SettleDebt { caller, amount } => {
            let req = SettleDebtRequest {
                caller: caller.clone(),
                amount: *amount,
            };
            ops::prepare_settle_debt(chain, v, &req, accounts, now)
        }
        Action::TakeSurplus(t) => ops::prepare_take_surplus(chain, v, &t.request()?, accounts, now),
        Action::TakeDebt(t) => ops::prepare_take_debt(chain, v, &t.request()?, accounts, now),
        Action::OraclePublish {
            price_source,
            price,
            host_admin,
            engine_admin,
            base_window_ms,
        } => {
            let req = OraclePublishRequest {
                price_source: price_source.clone(),
                price: *price,
                host_admin: host_admin.clone(),
                engine_admin: engine_admin.clone(),
                base_window_ms: *base_window_ms,
            };
            ops::prepare_oracle_publish(chain, &ctx.deployment, v, &req, accounts, now)
        }
        Action::DisableProtocolToken { admin } => {
            disable_protocol_token(chain, v, admin, accounts, now)
        }
        other => caller_action(chain, v, other, accounts, now),
    };
    Ok(p?)
}

/// The actions whose only argument is the caller.
fn caller_action(
    chain: &dyn Chain,
    v: &VerifiedConfig,
    action: &Action,
    accounts: &Accounts<'_>,
    now: u64,
) -> SdkResult<Prepared> {
    match action {
        Action::ObserveFeed { caller: c } => {
            ops::prepare_observe_feed(chain, v, &caller(c), accounts, now)
        }
        Action::CheckSolvency { caller: c } => {
            ops::prepare_check_solvency(chain, v, &caller(c), accounts, now)
        }
        Action::StartSurplus { caller: c } => {
            ops::prepare_start_surplus(chain, v, &caller(c), accounts, now)
        }
        Action::CloseSurplus { caller: c } => {
            ops::prepare_close_surplus(chain, v, &caller(c), accounts, now)
        }
        Action::StartDebt { caller: c } => {
            ops::prepare_start_debt(chain, v, &caller(c), accounts, now)
        }
        Action::CloseDebt { caller: c } => {
            ops::prepare_close_debt(chain, v, &caller(c), accounts, now)
        }
        _ => Err(SdkError::Refused(
            "internal: not a caller-only action".into(),
        )),
    }
}

/// The demo's freeze trigger (`admin set-engine-params --params cli/params/demo-engine.json
/// --disable-protocol-token`, scripts/lib/demo-scenario.sh): `ops::prepare_set_engine_params_with`
/// on `demo::engine_params()` (the same values as the file, pinned by the SDK's params test), no
/// protocol token, no ref oracle. Every check and fact is the SDK's.
fn disable_protocol_token<C: Chain + ?Sized>(
    chain: &C,
    v: &VerifiedConfig,
    admin: &str,
    accounts: &Accounts<'_>,
    now: u64,
) -> SdkResult<Prepared> {
    let slots = EngineParamsSlots {
        admin: admin.to_owned(),
        protocol_token: None,
        ref_oracle: None,
        disable_protocol_token: true,
        disable_ref_oracle: false,
    };
    let load = |token_enabled, ref_oracle_enabled| {
        let mut p = demo::engine_params();
        p.protocol_token_enabled = token_enabled;
        p.ref_oracle_enabled = ref_oracle_enabled;
        Ok(p)
    };
    ops::prepare_set_engine_params_with(chain, v, &slots, load, accounts, now)
}

/// A base58 account id, or a wallet label (which needs the wallet).
fn account(ctx: &Ctx, arg: &str) -> SdkResult<AccountId> {
    if let Ok(id) = arg.parse::<AccountId>() {
        return Ok(id);
    }
    let path = Keystore::default_path(ctx.wallet_home.as_deref())?;
    Ok(Keystore::open(&path)?.resolve(arg)?)
}

fn touch_json(t: &Touch) -> Value {
    match t {
        Touch::Holding {
            label,
            id,
            definition,
        } => json!({
            "kind": "holding",
            "label": label,
            "id": id.to_string(),
            "definition": definition.to_string(),
        }),
        Touch::Auction(id) => json!({ "kind": "auction", "id": id.to_string() }),
        Touch::Accounting => json!({ "kind": "accounting" }),
    }
}

/// A `Prepared` as the module consumes it.
#[must_use]
pub fn prepared_json(p: &Prepared) -> Value {
    let steps: Vec<Value> = p
        .steps
        .iter()
        .map(|s| plan_json(&s.plan, s.what, s.check))
        .collect();
    let facts: Vec<Value> = p
        .facts
        .iter()
        .map(|f| json!({ "key": f.key, "value": f.value }))
        .collect();
    let touched: Vec<Value> = p.touched.iter().map(touch_json).collect();
    json!({ "steps": steps, "facts": facts, "touched": touched })
}

/// The plan as the module consumes it: account ids as hex (the `lez_core` form), signers as base58
/// (the lock names).
#[must_use]
pub fn plan_json(plan: &Plan, what: &str, check: &str) -> Value {
    let accounts: Vec<Value> = plan
        .account_ids
        .iter()
        .map(|id| {
            json!({
                "id": hex::encode(id.value()),
                "signer": plan.signing_requirements.contains(id),
            })
        })
        .collect();
    let signers: Vec<String> = plan
        .signing_requirements
        .iter()
        .map(ToString::to_string)
        .collect();
    json!({
        "program_id": program_id_hex(&plan.program_id),
        "accounts": accounts,
        "instruction_words": plan.instruction_words,
        "signers": signers,
        "what": what,
        "check": check,
    })
}
