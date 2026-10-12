//! Signing: a plan plus the signers' current nonces become a `PublicTransaction`. The message is built
//! with `Message::new_preserialized` from the plan's own instruction words, so what is signed is
//! exactly what the encoders produced. Signatures are LEZ's own (`WitnessSet::for_message`), the
//! scheme the sequencer verifies with `WitnessSet::is_valid_for`.

use lee::{
    public_transaction::{Message, WitnessSet},
    PrivateKey, PublicTransaction,
};
use lee_core::account::{AccountId, Nonce};

use crate::{
    error::{SdkError, SdkResult},
    keystore::Keystore,
    plan::Plan,
};

pub trait KeySource {
    fn signing_key(&self, id: &AccountId) -> SdkResult<&PrivateKey>;
}

impl KeySource for Keystore {
    fn signing_key(&self, id: &AccountId) -> SdkResult<&PrivateKey> {
        self.key(id).map_err(SdkError::from)
    }
}

/// `nonces[i]` is the current nonce of `plan.signing_requirements[i]`.
pub fn sign(plan: &Plan, nonces: &[Nonce], keys: &dyn KeySource) -> SdkResult<PublicTransaction> {
    if nonces.len() != plan.signing_requirements.len() {
        return Err(SdkError::Encode(format!(
            "{} nonces for {} signers",
            nonces.len(),
            plan.signing_requirements.len()
        )));
    }
    let signing: Vec<&PrivateKey> = plan
        .signing_requirements
        .iter()
        .map(|id| keys.signing_key(id))
        .collect::<SdkResult<_>>()?;
    let message = Message::new_preserialized(
        plan.program_id,
        plan.account_ids.clone(),
        nonces.to_vec(),
        plan.instruction_words.clone(),
    );
    let witness = WitnessSet::for_message(&message, &signing);
    Ok(PublicTransaction::new(message, witness))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::KeystoreError;
    use crate::testing::standard::{self, k, ORACLE};
    use crate::testing::{id, keystore_json, TestKeys};
    use crate::verify::verify_config;
    use lee::PublicKey;
    use std::path::Path;

    fn caller_plan() -> Plan {
        let fx = standard::standard();
        let v = verify_config(&fx.chain, &fx.deployment, &standard::config_id()).expect("verifies");
        crate::plan::observe_feed(&v, id(k::CALLER), ORACLE).expect("plan")
    }

    #[test]
    fn sign_produces_a_witness_valid_for_the_message_in_signer_order() {
        let plan = caller_plan();
        let tx = sign(&plan, &[Nonce(3)], &TestKeys::of(&[k::CALLER])).expect("signed");
        assert!(tx.witness_set().is_valid_for(tx.message()));
        assert_eq!(tx.message().nonces, vec![Nonce(3)]);
        assert_eq!(tx.message().instruction_data, plan.instruction_words);
        let e = sign(&plan, &[Nonce(3)], &TestKeys::of(&[k::KEEPER])).expect_err("no key");
        assert!(matches!(e, SdkError::Keystore(KeystoreError::NoKey(_))));
        let e = sign(&plan, &[], &TestKeys::of(&[k::CALLER])).expect_err("nonce count");
        assert!(matches!(e, SdkError::Encode(_)));
    }

    /// The scheme the sequencer checks is `WitnessSet::is_valid_for` (Schnorr over the message hash,
    /// each signature against its own public key); a keystore-signed transaction must satisfy it, with
    /// the signer's public key deriving the plan's signer id, and fail for any other message.
    #[test]
    fn keystore_signed_transaction_verifies_with_lez_and_not_for_another_message() {
        let plan = caller_plan();
        let ks = Keystore::parse(
            &keystore_json(&[("caller", k::CALLER)]),
            Path::new("/w/storage.json"),
        )
        .expect("parses");
        let tx = sign(&plan, &[Nonce(7)], &ks).expect("signed");
        assert!(tx.witness_set().is_valid_for(tx.message()));
        let signers: Vec<AccountId> = tx
            .witness_set()
            .signatures_and_public_keys()
            .iter()
            .map(|(_, pk): &(_, PublicKey)| AccountId::from(pk))
            .collect();
        assert_eq!(signers, plan.signing_requirements);
        let other = sign(&plan, &[Nonce(8)], &ks).expect("signed");
        assert!(!tx.witness_set().is_valid_for(other.message()));
    }
}
