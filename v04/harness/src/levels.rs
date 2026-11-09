//! Pause levels on the [`World`] (spec §3.2, ADR-073): the admin's `SetPauseLevel` and a
//! snapshot of every actor a refused transaction must leave alone.

use engine_core::{
    msg::EngineMsg,
    state::{AccountingState, LiquidationRecordState},
};
use lee::{Actor, error::LeeError};
use lee_core::program::TransactionEvent;
use mock_host_core::vault_id;

use crate::{World, world::SALT};

impl World {
    /// `SetPauseLevel` of the world's collateral definition for salt [`SALT`] to `level`,
    /// claiming the world's current time.
    #[must_use]
    pub fn set_pause_level_msg(&self, level: u8) -> EngineMsg {
        EngineMsg::SetPauseLevel {
            salt: SALT,
            collateral_def: self.collateral_def,
            level,
            now_ms: self.now_ms,
        }
    }

    /// The admin sets the pause level to `level` through its proxy, signing.
    pub fn set_pause_level(&mut self, level: u8) -> Result<Vec<TransactionEvent>, LeeError> {
        let (key, msg) = (self.admin_key.clone(), self.set_pause_level_msg(level));
        self.send_engine(self.admin, &[&key], &msg)
    }

    /// The state of every engine actor and holding the world knows of: the config, accounting
    /// and collateral actors; each owner's record, host position and vault holdings; every
    /// auction of a counted record with its vault holdings; every surplus and debt auction the
    /// accounting counted with the surplus vault; the buffer; and the collateral, stablecoin and
    /// protocol holdings of every signer the chain has handed a key to.
    #[must_use]
    pub fn snapshot_all(&self) -> Vec<(Actor, Option<Vec<u8>>)> {
        let mut actors = vec![self.config, self.accounting, self.collateral, self.params];
        actors.extend(self.position_actors());
        actors.extend(self.system_auction_actors());
        let buffer = self.buffer().account_id;
        let defs = [self.collateral_def, self.stable_def, self.protocol_def()];
        let mut holders = self.chain.key_accounts();
        holders.push(buffer);
        for holder in holders {
            actors.extend(defs.iter().map(|def| self.holding(holder, *def)));
        }
        actors
            .into_iter()
            .map(|actor| (actor, self.chain.raw_state(actor)))
            .collect()
    }

    /// Each owner's record, host position and host vault holding, and every auction of its
    /// record with the auction vault's collateral holding.
    fn position_actors(&self) -> Vec<Actor> {
        let mut actors = Vec::new();
        for owner in &self.owners {
            let record = self.record(owner.position);
            actors.push(record);
            actors.push(Actor::new(owner.position, self.host_prog));
            let host_vault = vault_id(self.host_prog, owner.account);
            actors.push(self.holding(host_vault, self.collateral_def));
            let count = self
                .chain
                .raw_state(record)
                .and_then(|bytes| LiquidationRecordState::decode(&bytes))
                .map_or(0, |r| r.count);
            for nonce in 0..count {
                let vault = self.vault(owner.position, nonce);
                actors.push(self.auction(owner.position, nonce));
                actors.push(self.holding(vault.account_id, self.collateral_def));
            }
        }
        actors
    }

    /// Every surplus auction (with its vault's stablecoin holding) and debt auction the
    /// accounting has counted.
    fn system_auction_actors(&self) -> Vec<Actor> {
        let Some(acc) = self
            .chain
            .raw_state(self.accounting)
            .and_then(|bytes| AccountingState::decode(&bytes))
        else {
            return Vec::new();
        };
        let mut actors = Vec::new();
        for nonce in 0..acc.surplus_nonce {
            actors.push(self.surplus(nonce));
            let vault = self.surplus_vault(nonce).account_id;
            actors.push(self.holding(vault, self.stable_def));
        }
        actors.extend((0..acc.debt_nonce).map(|nonce| self.debt(nonce)));
        actors
    }
}
