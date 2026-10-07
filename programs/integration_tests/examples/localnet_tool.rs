//! Localnet helper (Plan 2 Task 20): export guest binaries and print the PDAs the smoke flow needs.
//!
//! Only the engine and the stablecoin are exported. The test-only attacker guests (impostor,
//! flawed engine) and the second stablecoin instance (`methods_b`) are never exported or deployed.
use std::{env, fs, path::PathBuf};

use auction_engine_core::pda;
use lee_core::{account::AccountId, program::ProgramId};
use stablecoin_core::{
    compute_liquidation_engine_state_pda, compute_position_pda, compute_position_vault_pda,
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, compute_stablecoin_definition_pda,
    compute_stablecoin_master_holding_pda,
};

fn hex_of(id: &ProgramId) -> String {
    id.iter()
        .flat_map(|l| l.to_le_bytes())
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn limbs_of(id: &ProgramId) -> String {
    id.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

fn fail(msg: &str) -> ! {
    eprintln!("localnet_tool: {msg}");
    std::process::exit(2);
}

/// Strict hex decoding: even length, ASCII hex digits only (no panics on bad input).
fn decode_hex(h: &str) -> Result<Vec<u8>, String> {
    if !h.is_ascii() {
        return Err("hex input contains non-ASCII characters".into());
    }
    if !h.len().is_multiple_of(2) {
        return Err(format!("hex input has odd length {}", h.len()));
    }
    (0..h.len() / 2)
        .map(|i| {
            u8::from_str_radix(&h[2 * i..2 * i + 2], 16)
                .map_err(|_| format!("invalid hex digits at offset {}", 2 * i))
        })
        .collect()
}

fn program_from_hex(h: &str) -> Result<ProgramId, String> {
    let bytes = decode_hex(h)?;
    if bytes.len() != 32 {
        return Err(format!(
            "ImageID must be 64 hex characters, got {}",
            h.len()
        ));
    }
    Ok(core::array::from_fn(|i| {
        u32::from_le_bytes([
            bytes[4 * i],
            bytes[4 * i + 1],
            bytes[4 * i + 2],
            bytes[4 * i + 3],
        ])
    }))
}

const USAGE: &str = "usage: localnet_tool export-guests <dir> | pdas <engine_hex> <host_hex> \
<engine_admin_base58> <salt> <owner_base58> <position_nonce> <collateral_def_base58> | \
decode-config <data_hex>";

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("export-guests") if args.len() == 2 => {
            let dir = PathBuf::from(&args[1]);
            fs::create_dir_all(&dir).expect("dir");
            fs::write(
                dir.join("stablecoin.bin"),
                stablecoin_methods::STABLECOIN_ELF,
            )
            .expect("write");
            fs::write(
                dir.join("auction_engine.bin"),
                auction_engine_methods::AUCTION_ENGINE_ELF,
            )
            .expect("write");
            println!(
                "STABLECOIN_ID {} limbs {}",
                hex_of(&stablecoin_methods::STABLECOIN_ID),
                limbs_of(&stablecoin_methods::STABLECOIN_ID)
            );
            println!(
                "AUCTION_ENGINE_ID {} limbs {}",
                hex_of(&auction_engine_methods::AUCTION_ENGINE_ID),
                limbs_of(&auction_engine_methods::AUCTION_ENGINE_ID)
            );
        }
        Some("pdas") if args.len() == 8 => {
            let engine = program_from_hex(&args[1]).unwrap_or_else(|e| fail(&e));
            let host = program_from_hex(&args[2]).unwrap_or_else(|e| fail(&e));
            let admin: AccountId = args[3].parse().expect("engine admin account id (base58)");
            let salt: u64 = args[4].parse().expect("salt");
            let owner: AccountId = args[5].parse().expect("owner account id (base58)");
            let nonce: u64 = args[6].parse().expect("position nonce");
            let collateral: AccountId = args[7].parse().expect("collateral definition (base58)");
            let config = pda::config_pda(&engine, &admin, salt);
            let position = compute_position_pda(host, owner, nonce);
            let auction = pda::auction_pda(&engine, &config, &position, 0);
            for (name, id) in [
                ("PP", compute_protocol_parameters_pda(host)),
                ("SFA", compute_stability_fee_accumulator_pda(host)),
                ("RPS", compute_redemption_price_state_pda(host)),
                ("SDEF", compute_stablecoin_definition_pda(host)),
                ("SMASTER", compute_stablecoin_master_holding_pda(host)),
                ("LES", compute_liquidation_engine_state_pda(host)),
                ("POSITION", position),
                ("HOST_VAULT", compute_position_vault_pda(host, position)),
                ("CONFIG", config),
                (
                    "COLL_CONFIG",
                    pda::coll_config_pda(&engine, &config, &collateral),
                ),
                ("ACCOUNTING", pda::accounting_pda(&engine, &config)),
                ("BUFFER", pda::buffer_pda(&engine, &config)),
                ("ENGINE_AUTH", pda::engine_authority_pda(&engine, &config)),
                ("UNUSED0", pda::unused_slot_pda(&engine, &config, 0)),
                ("UNUSED1", pda::unused_slot_pda(&engine, &config, 1)),
                ("LIQREC", pda::liqrec_pda(&engine, &config, &position)),
                ("AUCTION", auction),
                ("AUCTION_VAULT", pda::vault_pda(&engine, &auction)),
            ] {
                println!("{name}={id}");
            }
            println!("ENGINE_LIMBS={}", limbs_of(&engine));
        }
        Some("decode-config") if args.len() == 2 => {
            // Bind-time verification (ADR-027, I7): decode an EngineConfig account's raw data.
            let h = args[1].trim();
            let bytes = decode_hex(h).unwrap_or_else(|e| fail(&e));
            let data = lee_core::account::Data::try_from(bytes)
                .unwrap_or_else(|e| fail(&format!("not valid account data: {e:?}")));
            let cfg = auction_engine_core::state::EngineConfigState::try_from(&data)
                .unwrap_or_else(|e| fail(&format!("not an EngineConfig account: {e:?}")));
            println!("admin={} salt={}", cfg.admin, cfg.salt);
            println!(
                "host_program_id={} token_program_id={}",
                hex_of(&cfg.host_program_id),
                hex_of(&cfg.token_program_id)
            );
            println!(
                "host_protocol_parameters_id={} stablecoin_definition_id={}",
                cfg.host_protocol_parameters_id, cfg.stablecoin_definition_id
            );
            println!("{cfg:?}");
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}
