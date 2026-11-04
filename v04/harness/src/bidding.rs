//! Bids and leftover claims on the [`World`] (design note §4.2 rows 5-8).
//!
//! A `Bid` is a root message at the collateral actor, signed by the bidder: the bidder's token
//! actor `(bidder, T)` pays the charge from its stablecoin holding, and the collateral lands in its
//! collateral holding (ADR-062). A `ClaimLeftover` is a
//! root message at the owner's proxy `(owner, E)`, signed by the owner. A `SettleEmpty` is a
//! permissionless root message at the config.

use engine_core::{actors::buffer_holding, msg::EngineMsg};
use lee::{AccountId, Actor, PrivateKey, error::LeeError};
use lee_core::program::TransactionEvent;

use crate::World;

/// A funded bidder: a signer whose stablecoin holding holds stablecoin.
#[derive(Clone)]
pub struct Bidder {
    pub key: PrivateKey,
    pub account: AccountId,
}

impl World {
    /// The surplus buffer's token actor `(pda(E, ["BUFFER", config]), T)`; its stablecoin is
    /// `holding(buffer.account_id, stable_def)`.
    pub fn buffer(&self) -> Actor {
        buffer_holding(self.engine, self.token, self.config.account_id)
    }

    /// `account`'s stablecoin balance.
    pub fn stable_balance(&self, account: AccountId) -> u128 {
        self.balance(account, self.stable_def)
    }

    /// A fresh signer `label` with `stable` stablecoin in its own holding (minted by the host
    /// against a position of its own) and no collateral.
    pub fn fund_bidder(&mut self, label: &str, stable: u128) -> Bidder {
        let (key, account, _) = self.open_position(label, stable, stable);
        Bidder { key, account }
    }

    /// `bidder`'s `Bid` on auction `nonce` of `position`, claiming `now_ms`.
    pub fn bid_msg(
        &self,
        bidder: AccountId,
        position: AccountId,
        nonce: u64,
        (max_debt_in, min_collateral_out): (u128, u128),
        now_ms: u64,
    ) -> EngineMsg {
        EngineMsg::Bid {
            bidder,
            config: self.config.account_id,
            collateral_def: self.collateral_def,
            position,
            nonce,
            max_debt_in,
            min_collateral_out,
            now_ms,
        }
    }

    /// Every actor a bid by `bidder` on auction `nonce` of `position` touches: the collateral
    /// actor (root), host params, oracle, clock, the auction, the bidder's token actor and its
    /// stablecoin and collateral holdings, the stablecoin definition (burn), the buffer's
    /// stablecoin holding, the auction vault's token actor and collateral holding, the accounting
    /// and the config. No evidence: a fresh buffer holding is a token PDA its `Credit` grants.
    pub fn bid_actors(&self, bidder: AccountId, position: AccountId, nonce: u64) -> Vec<Actor> {
        let (cdef, sdef) = (self.collateral_def, self.stable_def);
        let vault = self.vault(position, nonce);
        vec![
            self.collateral,
            self.params,
            self.oracle,
            self.chain.clock(),
            self.auction(position, nonce),
            Actor::new(bidder, self.token),
            self.holding(bidder, sdef),
            self.holding(bidder, cdef),
            Actor::new(sdef, self.token),
            self.holding(self.buffer().account_id, sdef),
            vault,
            self.holding(vault.account_id, cdef),
            self.accounting,
            self.config,
        ]
    }

    /// Sends the `Bid` `msg` at the collateral actor, signed by `signers`.
    ///
    /// # Panics
    /// If `msg` is not a `Bid`.
    pub fn send_bid(
        &mut self,
        msg: &EngineMsg,
        signers: &[&PrivateKey],
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let EngineMsg::Bid {
            bidder,
            position,
            nonce,
            ..
        } = msg
        else {
            panic!("not a bid: {msg:?}")
        };
        let actors = self.bid_actors(*bidder, *position, *nonce);
        let root = self.collateral;
        self.chain.send(root, &actors, signers, msg)
    }

    /// `bidder` bids `(max_debt_in, min_collateral_out)` on auction `nonce` of `position` at the
    /// world's current time, signing.
    pub fn bid(
        &mut self,
        bidder: &Bidder,
        position: AccountId,
        nonce: u64,
        amounts: (u128, u128),
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let msg = self.bid_msg(bidder.account, position, nonce, amounts, self.now_ms);
        self.send_bid(&msg, &[&bidder.key])
    }

    /// `ClaimLeftover` of auction `nonce` of `position`.
    pub fn claim_msg(&self, position: AccountId, nonce: u64) -> EngineMsg {
        EngineMsg::ClaimLeftover {
            config: self.config.account_id,
            position,
            nonce,
        }
    }

    /// The claim of auction `nonce` of `position` sent at `claimant`'s proxy, signed by `signers`.
    pub fn claim_as(
        &mut self,
        claimant: AccountId,
        signers: &[&PrivateKey],
        position: AccountId,
        nonce: u64,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let msg = self.claim_msg(position, nonce);
        self.send_engine(claimant, signers, &msg)
    }

    /// The owner of `position` claims the leftover of its auction `nonce`, signing.
    pub fn claim(
        &mut self,
        position: AccountId,
        nonce: u64,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let owner = self.owner_of(position);
        let (account, key) = (owner.account, owner.key.clone());
        self.claim_as(account, &[&key], position, nonce)
    }

    /// `SettleEmpty` of auction `nonce` of `position`.
    pub fn settle_empty_msg(&self, position: AccountId, nonce: u64) -> EngineMsg {
        EngineMsg::SettleEmpty {
            config: self.config.account_id,
            position,
            nonce,
        }
    }

    /// The actors a `SettleEmpty` touches: the config (root), the auction, the accounting and the
    /// collateral actor.
    pub fn settle_empty_actors(&self, position: AccountId, nonce: u64) -> Vec<Actor> {
        vec![
            self.config,
            self.auction(position, nonce),
            self.accounting,
            self.collateral,
        ]
    }

    /// Anyone (no signature) settles the empty-lot auction `nonce` of `position`.
    pub fn settle_empty(
        &mut self,
        position: AccountId,
        nonce: u64,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let msg = self.settle_empty_msg(position, nonce);
        let actors = self.settle_empty_actors(position, nonce);
        let root = self.config;
        self.chain.send(root, &actors, &[], &msg)
    }

    /// The actors a claim at the proxy `(claimant, E)` touches, after the proxy: the auction, its
    /// vault's token actor and collateral holding, and the owner's collateral holding.
    pub(crate) fn claim_actors(&self, position: AccountId, nonce: u64) -> Vec<Actor> {
        let (owner, cdef) = (self.owner_of(position).account, self.collateral_def);
        let vault = self.vault(position, nonce);
        vec![
            self.auction(position, nonce),
            vault,
            self.holding(vault.account_id, cdef),
            self.holding(owner, cdef),
        ]
    }
}
