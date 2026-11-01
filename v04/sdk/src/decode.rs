//! Decoding actor states. The brief's `actor_state<T>(client, actor) -> Option<T>` returns
//! `Result<Option<T>, SdkError>` here so an RPC failure or an undecodable state is an error, not
//! a silent `None`.

use borsh::BorshDeserialize;
use lee::Actor;

use crate::{SdkError, simulate::StateSource};

/// `actor`'s state in `source` decoded as borsh `T`; `None` if absent.
///
/// # Errors
/// [`SdkError::MissingState`] if the state does not decode as `T`.
pub fn decode_state<T: BorshDeserialize>(
    source: &impl StateSource,
    actor: Actor,
) -> Result<Option<T>, SdkError> {
    source
        .actor_state(actor)
        .map(|bytes| decode(actor, &bytes))
        .transpose()
}

pub(crate) fn decode<T: BorshDeserialize>(actor: Actor, bytes: &[u8]) -> Result<T, SdkError> {
    borsh::from_slice(bytes)
        .map_err(|err| SdkError::MissingState(format!("state of {actor:?} does not decode: {err}")))
}

/// `actor`'s live state on the sequencer decoded as borsh `T`; `None` if absent.
///
/// # Errors
/// [`SdkError::Rpc`] if the sequencer fails; [`SdkError::MissingState`] if the state does not
/// decode as `T`.
pub async fn actor_state<T: BorshDeserialize>(
    client: &sequencer_service_rpc::SequencerClient,
    actor: Actor,
) -> Result<Option<T>, SdkError> {
    let snapshot = crate::live::Snapshot::fetch(client, [actor.account_id]).await?;
    decode_state(&snapshot, actor)
}
