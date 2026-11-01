//! Borsh encoding of the flat argument structs (feature `borsh`; only the v0.4 engine messages use
//! it). Hand-written rather than `cfg_attr(derive)` in `args.rs`: that file is shared verbatim with
//! the v0.2.4 guest, whose panic locations must not move (ruling R-F1). The encoding is exactly
//! what `#[derive(BorshSerialize, BorshDeserialize)]` produces: the fields in declaration order.

use borsh::{
    io::{Read, Result, Write},
    BorshDeserialize, BorshSerialize,
};

use crate::args::{CollateralArgs, EngineParamsArgs};

/// Implements both Borsh traits for a struct with all-public fields, field by field in the order
/// given (which must be the declaration order).
macro_rules! borsh_fields {
    ($ty:ident { $($field:ident),+ $(,)? }) => {
        impl BorshSerialize for $ty {
            fn serialize<W: Write>(&self, writer: &mut W) -> Result<()> {
                $(BorshSerialize::serialize(&self.$field, writer)?;)+
                Ok(())
            }
        }

        impl BorshDeserialize for $ty {
            fn deserialize_reader<R: Read>(reader: &mut R) -> Result<Self> {
                Ok(Self {
                    $($field: BorshDeserialize::deserialize_reader(reader)?,)+
                })
            }
        }
    };
}

borsh_fields!(EngineParamsArgs {
    global_cap,
    freeze_on_bad_debt,
    bad_debt_freeze_threshold,
    protocol_token_enabled,
    protocol_token_ref_price,
    ref_oracle_enabled,
    ref_oracle_max_age_ms,
    surplus_buffer_target,
    surplus_lot,
    surplus_delay_ms,
    surplus_premium_bps,
    surplus_max_discount_bps,
    debt_lot,
    debt_initial_discount_bps,
    debt_growth,
    surplus_decay,
    max_mint_per_auction,
    min_take,
    system_ttl_ms,
});

borsh_fields!(CollateralArgs {
    oracle_scale,
    max_oracle_age_ms,
    max_conf_bps,
    band_lo_bps,
    band_hi_bps,
    penalty_bps,
    tip,
    chip_bps,
    reward_cap,
    debt_floor,
    min_bid,
    type_cap,
    schedule,
    admin_paused,
});

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors with the derive, field for field: the hand-written impls must encode identically.
    #[derive(BorshSerialize)]
    struct EngineParamsMirror {
        global_cap: u128,
        freeze_on_bad_debt: bool,
        bad_debt_freeze_threshold: u128,
        protocol_token_enabled: bool,
        protocol_token_ref_price: u128,
        ref_oracle_enabled: bool,
        ref_oracle_max_age_ms: u64,
        surplus_buffer_target: u128,
        surplus_lot: u128,
        surplus_delay_ms: u64,
        surplus_premium_bps: u32,
        surplus_max_discount_bps: u32,
        debt_lot: u128,
        debt_initial_discount_bps: u32,
        debt_growth: Vec<u32>,
        surplus_decay: Vec<u32>,
        max_mint_per_auction: u128,
        min_take: u128,
        system_ttl_ms: u64,
    }

    #[derive(BorshSerialize)]
    struct CollateralMirror {
        oracle_scale: u8,
        max_oracle_age_ms: u64,
        max_conf_bps: u32,
        band_lo_bps: u32,
        band_hi_bps: u32,
        penalty_bps: u32,
        tip: u128,
        chip_bps: u32,
        reward_cap: u128,
        debt_floor: u128,
        min_bid: u128,
        type_cap: u128,
        schedule: Vec<u32>,
        admin_paused: bool,
    }

    fn engine_mirror(a: &EngineParamsArgs) -> EngineParamsMirror {
        EngineParamsMirror {
            global_cap: a.global_cap,
            freeze_on_bad_debt: a.freeze_on_bad_debt,
            bad_debt_freeze_threshold: a.bad_debt_freeze_threshold,
            protocol_token_enabled: a.protocol_token_enabled,
            protocol_token_ref_price: a.protocol_token_ref_price,
            ref_oracle_enabled: a.ref_oracle_enabled,
            ref_oracle_max_age_ms: a.ref_oracle_max_age_ms,
            surplus_buffer_target: a.surplus_buffer_target,
            surplus_lot: a.surplus_lot,
            surplus_delay_ms: a.surplus_delay_ms,
            surplus_premium_bps: a.surplus_premium_bps,
            surplus_max_discount_bps: a.surplus_max_discount_bps,
            debt_lot: a.debt_lot,
            debt_initial_discount_bps: a.debt_initial_discount_bps,
            debt_growth: a.debt_growth.clone(),
            surplus_decay: a.surplus_decay.clone(),
            max_mint_per_auction: a.max_mint_per_auction,
            min_take: a.min_take,
            system_ttl_ms: a.system_ttl_ms,
        }
    }

    fn collateral_mirror(a: &CollateralArgs) -> CollateralMirror {
        CollateralMirror {
            oracle_scale: a.oracle_scale,
            max_oracle_age_ms: a.max_oracle_age_ms,
            max_conf_bps: a.max_conf_bps,
            band_lo_bps: a.band_lo_bps,
            band_hi_bps: a.band_hi_bps,
            penalty_bps: a.penalty_bps,
            tip: a.tip,
            chip_bps: a.chip_bps,
            reward_cap: a.reward_cap,
            debt_floor: a.debt_floor,
            min_bid: a.min_bid,
            type_cap: a.type_cap,
            schedule: a.schedule.clone(),
            admin_paused: a.admin_paused,
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn engine_params_encode_like_the_derive_and_round_trip() {
        let args = EngineParamsArgs::defaults_for_tests();
        let bytes = borsh::to_vec(&args).expect("encodes");
        assert_eq!(
            bytes,
            borsh::to_vec(&engine_mirror(&args)).expect("encodes")
        );
        assert_eq!(
            borsh::from_slice::<EngineParamsArgs>(&bytes).expect("decodes"),
            args
        );
    }

    #[test]
    fn collateral_args_encode_like_the_derive_and_round_trip() {
        let args = CollateralArgs::defaults_for_tests();
        let bytes = borsh::to_vec(&args).expect("encodes");
        assert_eq!(
            bytes,
            borsh::to_vec(&collateral_mirror(&args)).expect("encodes")
        );
        assert_eq!(
            borsh::from_slice::<CollateralArgs>(&bytes).expect("decodes"),
            args
        );
    }

    /// Pinned bytes of one sample: field order, LE widths, `Vec` length prefix and bool byte.
    #[test]
    fn collateral_args_sample_bytes_are_pinned() {
        let args = CollateralArgs {
            oracle_scale: 1,
            max_oracle_age_ms: 2,
            max_conf_bps: 3,
            band_lo_bps: 4,
            band_hi_bps: 5,
            penalty_bps: 6,
            tip: 7,
            chip_bps: 8,
            reward_cap: 9,
            debt_floor: 10,
            min_bid: 11,
            type_cap: 12,
            schedule: vec![13, 14],
            admin_paused: true,
        };
        let want = concat!(
            "01",                               // oracle_scale u8
            "0200000000000000",                 // max_oracle_age_ms u64
            "03000000",                         // max_conf_bps u32
            "04000000",                         // band_lo_bps
            "05000000",                         // band_hi_bps
            "06000000",                         // penalty_bps
            "07000000000000000000000000000000", // tip u128
            "08000000",                         // chip_bps
            "09000000000000000000000000000000", // reward_cap
            "0a000000000000000000000000000000", // debt_floor
            "0b000000000000000000000000000000", // min_bid
            "0c000000000000000000000000000000", // type_cap
            "02000000",                         // schedule len
            "0d000000",
            "0e000000",
            "01", // admin_paused
        );
        let bytes = borsh::to_vec(&args).expect("encodes");
        assert_eq!(hex(&bytes), want);
        assert_eq!(
            borsh::from_slice::<CollateralArgs>(&bytes).expect("decodes"),
            args
        );
    }
}
