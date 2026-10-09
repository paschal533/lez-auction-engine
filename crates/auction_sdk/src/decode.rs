//! Decoders for every account a client reads. Engine accounts: initialised, owned by the engine, right
//! 8-byte tag (`TryFrom<&Data>` checks it). Host accounts: owned by the host program.

use auction_engine_core::state::{
    AccountingState, AuctionState, CollateralConfigState, DebtAuctionState, EngineConfigState,
    LiquidationRecordState, SurplusAuctionState,
};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::ProgramId,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::OraclePriceAccount;

use crate::{
    chain::Chain,
    deployment::{program_id_hex, Deployment},
    error::{SdkError, SdkResult},
};

pub struct Reader<'a, C: Chain + ?Sized> {
    chain: &'a C,
    engine: ProgramId,
    /// Expected owner of oracle accounts (the deployment's twap oracle program), when known.
    oracle_program: Option<ProgramId>,
}

fn decode_err(what: &'static str, id: &AccountId, e: &std::io::Error) -> SdkError {
    SdkError::Decode {
        what,
        id: id.to_string(),
        reason: e.to_string(),
    }
}

impl<'a, C: Chain + ?Sized> Reader<'a, C> {
    pub fn new(chain: &'a C, engine: ProgramId) -> Self {
        Self {
            chain,
            engine,
            oracle_program: None,
        }
    }

    /// A reader that also pins oracle accounts to the deployment's twap oracle program (when set).
    pub fn for_deployment(chain: &'a C, d: &Deployment) -> Self {
        Self {
            chain,
            engine: d.engine_program,
            oracle_program: d.twap_oracle_program,
        }
    }

    fn initialised(&self, id: &AccountId, what: &'static str) -> SdkResult<Account> {
        let acc = self.chain.account(id)?;
        if acc == Account::default() {
            return Err(SdkError::Missing {
                what,
                id: id.to_string(),
            });
        }
        Ok(acc)
    }

    fn owned<T>(&self, id: &AccountId, owner: &ProgramId, what: &'static str) -> SdkResult<T>
    where
        T: for<'d> TryFrom<&'d Data, Error = std::io::Error>,
    {
        let acc = self.initialised(id, what)?;
        if acc.program_owner != *owner {
            return Err(SdkError::Mismatch {
                what,
                expected: format!("owner {}", program_id_hex(owner)),
                found: format!("owner {}", program_id_hex(&acc.program_owner)),
            });
        }
        T::try_from(&acc.data).map_err(|e| decode_err(what, id, &e))
    }

    pub fn config(&self, id: &AccountId) -> SdkResult<EngineConfigState> {
        self.owned(id, &self.engine, "EngineConfig")
    }

    pub fn coll_config(&self, id: &AccountId) -> SdkResult<CollateralConfigState> {
        self.owned(id, &self.engine, "CollateralConfig")
    }

    pub fn accounting(&self, id: &AccountId) -> SdkResult<AccountingState> {
        self.owned(id, &self.engine, "Accounting")
    }

    pub fn auction(&self, id: &AccountId) -> SdkResult<AuctionState> {
        self.owned(id, &self.engine, "Auction")
    }

    pub fn surplus_auction(&self, id: &AccountId) -> SdkResult<SurplusAuctionState> {
        self.owned(id, &self.engine, "SurplusAuction")
    }

    pub fn debt_auction(&self, id: &AccountId) -> SdkResult<DebtAuctionState> {
        self.owned(id, &self.engine, "DebtAuction")
    }

    /// `LiquidationRecord.count` (the next liquidation nonce of a position); 0 before the first.
    pub fn record_count(&self, id: &AccountId) -> SdkResult<u64> {
        if self.chain.account(id)? == Account::default() {
            return Ok(0);
        }
        let r: LiquidationRecordState = self.owned(id, &self.engine, "LiquidationRecord")?;
        Ok(r.count)
    }

    /// A host-owned account (`Position`, `ProtocolParameters`, `StabilityFeeAccumulator`,
    /// `RedemptionPriceState`, `LiquidationEngineState`).
    pub fn host<T>(&self, id: &AccountId, host: &ProgramId, what: &'static str) -> SdkResult<T>
    where
        T: for<'d> TryFrom<&'d Data, Error = std::io::Error>,
    {
        self.owned(id, host, what)
    }

    /// The engine pins the oracle id. When the reader knows the twap oracle program (`for_deployment`
    /// with `twap_oracle_program` set) the account must be owned by it; otherwise the owner is not
    /// checked (the on-chain engine pins the id).
    pub fn oracle(&self, id: &AccountId) -> SdkResult<OraclePriceAccount> {
        let acc = self.initialised(id, "OraclePriceAccount")?;
        if let Some(expected) = self.oracle_program {
            if acc.program_owner != expected {
                return Err(SdkError::Mismatch {
                    what: "oracle account owner (twap oracle program)",
                    expected: program_id_hex(&expected),
                    found: program_id_hex(&acc.program_owner),
                });
            }
        }
        OraclePriceAccount::try_from(&acc.data)
            .map_err(|e| decode_err("OraclePriceAccount", id, &e))
    }

    /// A token definition owned by the deployment's token program.
    pub fn token_definition(
        &self,
        id: &AccountId,
        token: &ProgramId,
    ) -> SdkResult<TokenDefinition> {
        let acc = self.initialised(id, "TokenDefinition")?;
        if acc.program_owner != *token {
            return Err(SdkError::Mismatch {
                what: "token definition owner (token program)",
                expected: program_id_hex(token),
                found: program_id_hex(&acc.program_owner),
            });
        }
        TokenDefinition::try_from(&acc.data).map_err(|e| decode_err("TokenDefinition", id, &e))
    }

    /// Fungible balance; `None` for an uninitialised (fresh) holding.
    pub fn balance(
        &self,
        id: &AccountId,
        token: &ProgramId,
        definition: &AccountId,
    ) -> SdkResult<Option<u128>> {
        let acc = self.chain.account(id)?;
        if acc == Account::default() {
            return Ok(None);
        }
        if acc.program_owner != *token {
            return Err(SdkError::Mismatch {
                what: "holding owner (token program)",
                expected: program_id_hex(token),
                found: program_id_hex(&acc.program_owner),
            });
        }
        match TokenHolding::try_from(&acc.data) {
            Ok(TokenHolding::Fungible {
                definition_id,
                balance,
            }) if definition_id == *definition => Ok(Some(balance)),
            Ok(TokenHolding::Fungible { definition_id, .. }) => Err(SdkError::Mismatch {
                what: "holding definition",
                expected: definition.to_string(),
                found: definition_id.to_string(),
            }),
            Ok(_) => Err(SdkError::Decode {
                what: "fungible holding",
                id: id.to_string(),
                reason: "not a fungible holding".into(),
            }),
            Err(e) => Err(decode_err("fungible holding", id, &e)),
        }
    }

    /// The account as the handlers see it (read-only, unsigned), for `auction_engine_program::feed`.
    pub fn meta(&self, id: &AccountId) -> SdkResult<AccountWithMetadata> {
        Ok(AccountWithMetadata::new(
            self.chain.account(id)?,
            false,
            *id,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::id;
    use crate::testing::standard::{self, k, COLLATERAL, ENGINE, TOKEN};

    #[test]
    fn reader_checks_initialisation_owner_tag_and_definition() {
        let fx = standard::standard();
        let r = Reader::new(&fx.chain, ENGINE);
        let cfg = r.config(&standard::config_id()).expect("config decodes");
        assert_eq!(cfg.salt, standard::SALT);
        assert!(matches!(r.config(&id(99)), Err(SdkError::Missing { .. })));
        // An accounting account read as a config: right owner, wrong tag.
        assert!(matches!(
            r.config(&fx.engine.accounting),
            Err(SdkError::Decode { .. })
        ));
        // A host-owned account read as an engine account.
        assert!(matches!(
            r.config(&fx.host.protocol_parameters),
            Err(SdkError::Mismatch { .. })
        ));
        assert_eq!(
            r.record_count(&fx.engine.record(&fx.unsafe_position))
                .expect("fresh"),
            0
        );
        assert_eq!(
            r.record_count(&fx.engine.record(&fx.auction_position))
                .expect("one"),
            1
        );
        assert_eq!(
            r.balance(
                &id(k::BIDDER_STABLE),
                &TOKEN,
                &fx.host.stablecoin_definition
            )
            .expect("ok"),
            Some(1_000_000)
        );
        assert!(matches!(
            r.balance(&id(k::BIDDER_STABLE), &TOKEN, &COLLATERAL),
            Err(SdkError::Mismatch { .. })
        ));
        assert_eq!(
            r.balance(&id(98), &TOKEN, &COLLATERAL).expect("fresh"),
            None
        );
    }

    #[test]
    fn oracle_and_token_definition_owners_are_checked() {
        let fx = standard::standard();
        let r = Reader::for_deployment(&fx.chain, &fx.deployment);
        // The fixture oracle is owned by ORACLE_OWNER, not by the deployment's twap program.
        assert!(matches!(
            r.oracle(&standard::ORACLE),
            Err(SdkError::Mismatch { .. })
        ));
        let mut d = fx.deployment.clone();
        d.twap_oracle_program = Some(standard::ORACLE_OWNER);
        let ok = Reader::for_deployment(&fx.chain, &d);
        assert_eq!(
            ok.oracle(&standard::ORACLE).expect("owned").price,
            standard::R
        );
        // No twap program in the deployment: owner unchecked.
        d.twap_oracle_program = None;
        let any = Reader::for_deployment(&fx.chain, &d);
        assert!(any.oracle(&standard::ORACLE).is_ok());

        let def = fx.host.stablecoin_definition;
        assert!(r.token_definition(&def, &TOKEN).is_ok());
        assert!(matches!(
            r.token_definition(&def, &[0xB2; 8]),
            Err(SdkError::Mismatch { .. })
        ));
        assert!(matches!(
            r.token_definition(&id(98), &TOKEN),
            Err(SdkError::Missing { .. })
        ));
    }
}
