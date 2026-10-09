//! Plans (spec §6.1): `{program_id, account_ids, signing_requirements, instruction_words}`. A plan is
//! everything a signer needs; nothing is signed here.

use std::collections::BTreeSet;

use lee_core::{account::AccountId, program::ProgramId};
use serde::Serialize;

use crate::{
    encode::words,
    error::{SdkError, SdkResult},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub program_id: ProgramId,
    pub account_ids: Vec<AccountId>,
    /// Accounts that must sign, in witness order; the message nonces follow the same order.
    pub signing_requirements: Vec<AccountId>,
    pub instruction_words: Vec<u32>,
}

impl Plan {
    /// Rejects what LEZ would reject anyway: a duplicate account id, a signer that is not one of the
    /// accounts, or no signer at all (a public transaction needs one).
    pub fn new<T: Serialize>(
        program_id: ProgramId,
        account_ids: Vec<AccountId>,
        signing_requirements: Vec<AccountId>,
        ix: &T,
    ) -> SdkResult<Self> {
        let mut seen = BTreeSet::new();
        for a in &account_ids {
            if !seen.insert(*a) {
                return Err(SdkError::Encode(format!(
                    "account {a} is listed twice (LEZ rejects duplicate account ids)"
                )));
            }
        }
        if signing_requirements.is_empty() {
            return Err(SdkError::Encode(
                "a public transaction needs a signer".into(),
            ));
        }
        for s in &signing_requirements {
            if !seen.contains(s) {
                return Err(SdkError::Encode(format!(
                    "signer {s} is not one of the plan's accounts"
                )));
            }
        }
        Ok(Self {
            program_id,
            account_ids,
            signing_requirements,
            instruction_words: words(ix)?,
        })
    }

    #[must_use]
    pub fn variant_index(&self) -> Option<u32> {
        self.instruction_words.first().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use auction_engine_core::Instruction;
    use lee_core::account::AccountId;

    const P: ProgramId = [7; 8];

    #[test]
    fn plan_rejects_duplicates_unlisted_signers_and_no_signer() {
        let (a, b) = (AccountId::new([1; 32]), AccountId::new([2; 32]));
        let ix = Instruction::ObserveFeed;
        assert!(matches!(
            Plan::new(P, vec![a, a], vec![a], &ix),
            Err(SdkError::Encode(m)) if m.contains("listed twice")
        ));
        assert!(matches!(
            Plan::new(P, vec![a], vec![b], &ix),
            Err(SdkError::Encode(m)) if m.contains("not one of")
        ));
        assert!(matches!(
            Plan::new(P, vec![a], vec![], &ix),
            Err(SdkError::Encode(_))
        ));
        let ok = Plan::new(P, vec![a, b], vec![a], &ix).expect("valid");
        assert_eq!(ok.variant_index(), Some(3));
    }
}
