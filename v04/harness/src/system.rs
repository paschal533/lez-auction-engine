//! System operations on the [`World`] (design note §4.2 rows 9-16): every system root is a
//! permissionless message at a config `K = (config, E)`, which hands the operation to its
//! accounting; the accounting reads the buffer's stablecoin holding (and the reference oracle).
//!
//! A buffer holding that was never credited (no bid has paid a penalty yet) is an absent actor:
//! the transaction admits it only with `Pda` evidence for the token program's holding seed
//! ([`World::buffer_evidence`]), and it then reads as 0.

use engine_core::{
    actors::{accounting_actor, buffer_holding},
    msg::EngineMsg,
    state::AccountingState,
};
use lee::{AccountId, Actor, PrivateKey, PublicAccountEvidence, error::LeeError};
use lee_core::program::TransactionEvent;
use mock_host_core::Params;
use token_core::{TokenMsg, holding_seed};

use crate::World;

/// Collateral and debt of the position whose empty lot books a shortfall (Plan 5's lot-3 case):
/// the keeper reward takes the whole 3-unit seizure.
const SHORTFALL_COLLATERAL: u128 = 3;
const SHORTFALL_DEBT: u128 = 10_000;

impl World {
    /// A fresh funder gets `amount` of stablecoin minted by the host (against a position of its
    /// own) and transfers it to `to`'s stablecoin holding.
    pub fn donate_stable(&mut self, to: AccountId, amount: u128) {
        let label = (0..)
            .map(|n| format!("donor-{n}"))
            .find(|label| {
                let (_, account) = self.chain.key(label);
                !self.chain.account_exists(account)
            })
            .expect("a free donor label");
        let funder = self.fund_bidder(&label, amount);
        let sdef = self.stable_def;
        let from = Actor::new(funder.account, self.token);
        let transfer = TokenMsg::Transfer {
            definition: sdef,
            to,
            amount,
        };
        let actors = [
            from,
            self.holding(funder.account, sdef),
            self.holding(to, sdef),
        ];
        self.chain
            .send(from, &actors, &[&funder.key], &transfer)
            .expect("the funder donates");
    }

    /// Plan 5's lot-3 `SettleEmpty` case: a fresh 3 / 10_000 position, liquidated at the host's
    /// redemption price (the keeper reward takes the whole seizure), then settled empty, which
    /// books its debt as a shortfall. Returns the accounting's new `unbacked_debt`.
    pub fn book_shortfall(&mut self) -> u128 {
        let redemption = self
            .chain
            .raw_state(self.params)
            .and_then(|bytes| Params::decode(&bytes))
            .expect("the host's params")
            .redemption_price_ray;
        let position = self.open_unsafe_position(SHORTFALL_COLLATERAL, SHORTFALL_DEBT);
        self.set_price(redemption);
        self.liquidate(position, 0)
            .expect("the reward takes the lot");
        self.settle_empty(position, 0)
            .expect("the empty lot settles");
        self.accounting_state().unbacked_debt
    }

    /// `SettleDebt { amount }` at the world's config.
    pub fn settle_debt_msg(&self, amount: u128) -> EngineMsg {
        EngineMsg::SettleDebt {
            config: self.config.account_id,
            amount,
        }
    }

    /// The actors a system root touches: for `SettleDebt` at `config`, the config (root), its
    /// accounting, the buffer's stablecoin holding (read), the buffer's token actor and the
    /// stablecoin definition (burn).
    ///
    /// # Panics
    /// For a message that is not a system root.
    pub fn system_actors(&self, msg: &EngineMsg) -> Vec<Actor> {
        let EngineMsg::SettleDebt { config, .. } = msg else {
            panic!("not a system root: {msg:?}")
        };
        let buffer = buffer_holding(self.engine, self.token, *config);
        vec![
            Actor::new(*config, self.engine),
            accounting_actor(self.engine, *config),
            self.holding(buffer.account_id, self.stable_def),
            buffer,
            Actor::new(self.stable_def, self.token),
        ]
    }

    /// Sends the system root `msg` at its config with `evidence`, signed by the signer `signer`
    /// (funded if new), or by no one if `signer` is empty.
    pub fn send_system(
        &mut self,
        msg: &EngineMsg,
        signer: &str,
        evidence: &[PublicAccountEvidence],
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let actors = self.system_actors(msg);
        let root = *actors.first().expect("the root config");
        let key: Option<PrivateKey> = (!signer.is_empty()).then(|| {
            let (key, account) = self.chain.key(signer);
            if !self.chain.account_exists(account) {
                self.chain.fund(account, 1);
            }
            key
        });
        let signers: Vec<&PrivateKey> = key.iter().collect();
        self.chain
            .send_with_evidence(root, &actors, &signers, evidence, msg)
    }

    /// The world's accounting state.
    pub fn accounting_state(&self) -> AccountingState {
        self.chain
            .raw_state(self.accounting)
            .and_then(|bytes| AccountingState::decode(&bytes))
            .expect("an accounting state")
    }

    /// The evidence a system root of the world's config needs: `Pda` for the buffer's stablecoin
    /// holding (the token program's `HOLDING` seed) while that holding is absent, else none.
    pub fn buffer_evidence(&self) -> Vec<PublicAccountEvidence> {
        let buffer = self.buffer().account_id;
        if self
            .chain
            .raw_state(self.holding(buffer, self.stable_def))
            .is_some()
        {
            return Vec::new();
        }
        vec![PublicAccountEvidence::Pda {
            program: self.token,
            seed: holding_seed(buffer, self.stable_def),
        }]
    }
}
