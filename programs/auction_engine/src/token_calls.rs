//! Chained token calls. lee requires each chained call's pre-states to equal the CURRENT state
//! (`InconsistentAccountPreState`) and each `is_authorized` flag to equal "signer, or a PDA whose seed
//! this call attaches" (`InvalidAccountAuthorization` / `AuthorizedAccountMarkedAsNotAuthorized`).
//! So a later call carries the simulated post-state of an earlier one (`with_balance`, `post_init`),
//! and a PDA is flagged authorised only in the call that attaches its seed.

use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{ChainedCall, PdaSeed, ProgramId},
};
use token_core::{Instruction as TokenIx, TokenHolding};

#[must_use]
pub fn authorized(acc: &AccountWithMetadata) -> AccountWithMetadata {
    let mut a = acc.clone();
    a.is_authorized = true;
    a
}

/// The holding as the token program will have written it with `balance` (same borsh encoding).
#[must_use]
pub fn with_balance(
    acc: &AccountWithMetadata,
    definition: AccountId,
    balance: u128,
) -> AccountWithMetadata {
    let mut a = acc.clone();
    a.account.data = Data::from(&TokenHolding::Fungible {
        definition_id: definition,
        balance,
    });
    a
}

/// A fresh account right after `Token::InitializeAccount` (like `open_position`'s `post_init_vault`).
#[must_use]
pub fn post_init(
    acc: &AccountWithMetadata,
    token_program: ProgramId,
    definition: AccountId,
) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: token_program,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: definition,
                balance: 0,
            }),
            nonce: acc.account.nonce,
        },
        is_authorized: false,
        account_id: acc.account_id,
    }
}

#[must_use]
pub fn transfer(
    token: ProgramId,
    from: AccountWithMetadata,
    to: AccountWithMetadata,
    amount: u128,
    seeds: Vec<PdaSeed>,
) -> ChainedCall {
    ChainedCall::new(
        token,
        vec![from, to],
        &TokenIx::Transfer {
            amount_to_transfer: amount,
        },
    )
    .with_pda_seeds(seeds)
}

#[must_use]
pub fn burn(
    token: ProgramId,
    definition: AccountWithMetadata,
    holding: AccountWithMetadata,
    amount: u128,
    seeds: Vec<PdaSeed>,
) -> ChainedCall {
    ChainedCall::new(
        token,
        vec![definition, holding],
        &TokenIx::Burn {
            amount_to_burn: amount,
        },
    )
    .with_pda_seeds(seeds)
}

#[must_use]
pub fn mint_with_authority(
    token: ProgramId,
    definition: AccountWithMetadata,
    holding: AccountWithMetadata,
    authority: AccountWithMetadata,
    amount: u128,
    seeds: Vec<PdaSeed>,
) -> ChainedCall {
    ChainedCall::new(
        token,
        vec![definition, holding, authority],
        &TokenIx::MintWithAuthority {
            amount_to_mint: amount,
        },
    )
    .with_pda_seeds(seeds)
}

#[must_use]
pub fn initialize_account(
    token: ProgramId,
    definition: AccountWithMetadata,
    holding: AccountWithMetadata,
    seeds: Vec<PdaSeed>,
) -> ChainedCall {
    ChainedCall::new(
        token,
        vec![definition, holding],
        &TokenIx::InitializeAccount,
    )
    .with_pda_seeds(seeds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lee_core::account::Nonce;

    #[test]
    fn simulated_states_use_the_token_encoding() {
        let id = AccountId::new([3; 32]);
        let def = AccountId::new([4; 32]);
        let fresh = AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: id,
        };
        let p = post_init(&fresh, [6; 8], def);
        assert_eq!(p.account.program_owner, [6; 8]);
        assert!(!p.is_authorized);
        assert_eq!(
            TokenHolding::try_from(&p.account.data).unwrap(),
            TokenHolding::Fungible {
                definition_id: def,
                balance: 0
            }
        );
        let q = with_balance(&p, def, 77);
        assert_eq!(
            TokenHolding::try_from(&q.account.data).unwrap(),
            TokenHolding::Fungible {
                definition_id: def,
                balance: 77
            }
        );
        assert_eq!(q.account.nonce, Nonce(0));
    }
}
