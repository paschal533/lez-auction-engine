//! Spec §3.2/§5.10: IDL position == enum index for every engine instruction (Shared Reference S1), and
//! the pinned IDL-position -> enum-index table of the vendored stablecoin. The IDL-name extraction, the
//! stablecoin samples and the table live once, in `auction_sdk` (`testing::support`, `encode`).

use auction_sdk::encode::{stablecoin_name, STABLECOIN_IDL_POSITION_TO_ENUM};
use auction_sdk::testing::support::{idl_instruction_names, stablecoin_samples};

fn word0<T: serde::Serialize>(ix: &T) -> u32 {
    risc0_zkvm::serde::to_vec(ix).expect("serializes")[0]
}

#[test]
fn every_engine_variant_has_a_sample_and_an_idl_entry_at_its_enum_index() {
    let samples = auction_engine_core::instruction::instruction_samples();
    let names = idl_instruction_names("auction_engine-idl.json");
    assert_eq!(
        samples.len(),
        names.len(),
        "one sample per IDL instruction (regenerate the IDL after adding a variant)"
    );
    for (i, ix) in samples.iter().enumerate() {
        assert_eq!(
            usize::try_from(word0(ix)).expect("u32 fits usize"),
            i,
            "sample {i} is variant {i}"
        );
        assert_eq!(ix.name(), names[i], "IDL position {i}");
    }
}

/// SDK encoders key on the enum index, never on the IDL position: for IDL positions 8..=18 the two
/// differ (the vendored IDL follows the upstream guest function order). `idl/stablecoin-idl.json` is
/// pinned to upstream and is not reordered.
#[test]
fn stablecoin_idl_position_maps_to_enum_index() {
    let idl = idl_instruction_names("stablecoin-idl.json");
    let samples = stablecoin_samples();
    let mut sample_names: Vec<&str> = samples.iter().map(stablecoin_name).collect();
    sample_names.sort_unstable();
    let mut idl_sorted: Vec<&str> = idl.iter().map(String::as_str).collect();
    idl_sorted.sort_unstable();
    assert_eq!(
        sample_names, idl_sorted,
        "every variant has an IDL entry and every IDL entry has a sample"
    );
    assert_eq!(idl.len(), STABLECOIN_IDL_POSITION_TO_ENUM.len());
    for (position, name) in idl.iter().enumerate() {
        let ix = samples
            .iter()
            .find(|s| stablecoin_name(s) == name)
            .expect("sample exists (checked above)");
        assert_eq!(
            word0(ix),
            STABLECOIN_IDL_POSITION_TO_ENUM[position],
            "IDL position {position} ({name})"
        );
    }
    let mut seen: Vec<u32> = samples.iter().map(word0).collect();
    seen.sort_unstable();
    assert_eq!(
        seen,
        (0..22).collect::<Vec<u32>>(),
        "enum indices 0..=21, each once"
    );
}
