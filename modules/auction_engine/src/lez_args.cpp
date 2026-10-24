#include "lez_args.h"

#include <stdexcept>

namespace {

using json = nlohmann::json;

constexpr std::int64_t MAX_WORD = 0xFFFFFFFFLL;
constexpr int BYTES_PER_WORD = 4;
constexpr int BITS_PER_BYTE = 8;

const json& field(const json& object, const char* key) {
    if (!object.is_object() || !object.contains(key)) {
        throw std::invalid_argument(std::string("plan step lacks \"") + key + "\"");
    }
    return object.at(key);
}

std::string textField(const json& object, const char* key) {
    const json& value = field(object, key);
    if (!value.is_string()) {
        throw std::invalid_argument(std::string("plan step \"") + key + "\" is not a string");
    }
    return value.get<std::string>();
}

const json& arrayField(const json& object, const char* key) {
    const json& value = field(object, key);
    if (!value.is_array()) {
        throw std::invalid_argument(std::string("plan step \"") + key + "\" is not an array");
    }
    return value;
}

void appendWord(std::vector<std::uint8_t>& bytes, const json& word) {
    if (!word.is_number_integer()) throw std::invalid_argument("instruction word is not an integer");
    const std::int64_t value = word.get<std::int64_t>();
    if (value < 0 || value > MAX_WORD) {
        throw std::invalid_argument("instruction word does not fit in 32 bits");
    }
    for (int i = 0; i < BYTES_PER_WORD; ++i) {
        bytes.push_back(static_cast<std::uint8_t>((value >> (BITS_PER_BYTE * i)) & 0xFF));
    }
}

}  // namespace

LezArgs toLezArgs(const json& step) {
    LezArgs args;
    args.programIdHex = textField(step, "program_id");
    for (const json& account : arrayField(step, "accounts")) {
        args.accountIds.push_back(textField(account, "id"));
        const json& signer = field(account, "signer");
        if (!signer.is_boolean()) throw std::invalid_argument("account \"signer\" is not a boolean");
        args.signers.push_back(signer.get<bool>());
    }
    for (const json& word : arrayField(step, "instruction_words")) appendWord(args.instruction, word);
    return args;
}
