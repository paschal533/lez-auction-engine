#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

// The arguments of `lez_core.send_generic_public_transaction` for one step of an `auction_ffi_prepare` plan.
struct LezArgs {
    std::vector<std::string> accountIds;
    std::vector<bool> signers;
    std::vector<std::uint8_t> instruction;  // the u32 words, each as 4 little-endian bytes
    std::string programIdHex;
};

/// `step` is one entry of `plan.steps`: `program_id`, `accounts [{id, signer}]`, `instruction_words [u32]`.
/// Throws std::invalid_argument when a field is missing or mistyped, or a word does not fit in 32 bits.
LezArgs toLezArgs(const nlohmann::json& step);
