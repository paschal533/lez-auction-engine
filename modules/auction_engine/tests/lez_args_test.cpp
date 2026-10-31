#include <cstdint>
#include <vector>

#include <logos_test.h>
#include <nlohmann/json.hpp>

#include "lez_args.h"

namespace {

using json = nlohmann::json;

const std::string PROGRAM_HEX(64, 'a');
const std::string ACCOUNT_A(64, '1');
const std::string ACCOUNT_B(64, '2');

json step(const json& words) {
    return {
        {"program_id", PROGRAM_HEX},
        {"accounts", json::array({{{"id", ACCOUNT_A}, {"signer", true}},
                                  {{"id", ACCOUNT_B}, {"signer", false}}})},
        {"instruction_words", words},
    };
}

}  // namespace

LOGOS_TEST(words_become_little_endian_bytes) {
    const LezArgs args = toLezArgs(step(json::array({1, 0x01020304})));

    const std::vector<std::uint8_t> expected = {0x01, 0x00, 0x00, 0x00, 0x04, 0x03, 0x02, 0x01};
    LOGOS_ASSERT_TRUE(args.instruction == expected);
    LOGOS_ASSERT_EQ(args.programIdHex, PROGRAM_HEX);
}

LOGOS_TEST(signer_flags_follow_accounts) {
    const LezArgs args = toLezArgs(step(json::array({7})));

    LOGOS_ASSERT_EQ(args.accountIds.size(), static_cast<std::size_t>(2));
    LOGOS_ASSERT_EQ(args.accountIds[0], ACCOUNT_A);
    LOGOS_ASSERT_EQ(args.accountIds[1], ACCOUNT_B);
    LOGOS_ASSERT_TRUE(args.signers == std::vector<bool>({true, false}));
}

LOGOS_TEST(a_word_above_u32_is_refused) {
    LOGOS_ASSERT_THROWS(toLezArgs(step(json::array({4294967296LL}))));
}

LOGOS_TEST(a_step_without_accounts_is_refused) {
    json broken = step(json::array({1}));
    broken.erase("accounts");
    LOGOS_ASSERT_THROWS(toLezArgs(broken));
}
