//! Spike S1 helper: print the five stablecoin global PDAs for a given ProgramId.
//! Copy to `lez-programs/programs/stablecoin/examples/global_pdas.rs`, then:
//!   cargo run -q -p stablecoin_program --example global_pdas -- <8 comma-separated u32 limbs>
use lee_core::program::ProgramId;
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, compute_stablecoin_definition_pda,
    compute_stablecoin_master_holding_pda,
};

fn main() {
    let arg = std::env::args().nth(1).expect("usage: global_pdas <8 u32 limbs, comma separated>");
    let limbs: Vec<u32> = arg.split(',').map(|x| x.trim().parse().expect("u32 limb")).collect();
    assert_eq!(limbs.len(), 8, "ProgramId must be 8 u32 limbs");
    let mut pid: ProgramId = [0u32; 8];
    pid.copy_from_slice(&limbs);
    println!("protocol_parameters       {}", compute_protocol_parameters_pda(pid));
    println!("stability_fee_accumulator {}", compute_stability_fee_accumulator_pda(pid));
    println!("redemption_price_state    {}", compute_redemption_price_state_pda(pid));
    println!("stablecoin_definition     {}", compute_stablecoin_definition_pda(pid));
    println!("stablecoin_master_holding {}", compute_stablecoin_master_holding_pda(pid));
}
