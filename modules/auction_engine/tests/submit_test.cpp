#include "auction_engine_impl.h"

#include <deque>
#include <string>

#include <QVariant>
#include <QVariantList>
#include <logos_test.h>
#include <nlohmann/json.hpp>

#include "budget.h"
#include "logos_sdk.h"
#include "mock_ffi_state.h"

namespace {

using json = nlohmann::json;

const std::string SIGNER_A = "SignerAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const std::string SIGNER_B = "SignerBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
const std::string SEND = "send_generic_public_transaction";
const std::int64_t MODULE_BUDGET_MS = 15000;
const std::int64_t VIEW_OVERRUN_MS = 11000;  // view / prepare: a 5 s read, a 1 s pause, another 5 s read
const std::int64_t TX_OVERRUN_MS = 5000;     // tx_begin / tx_poll: one 5 s read

std::string ok(const json& value) {
    return json{{"ok", true}, {"value", value}}.dump();
}

json step(const std::string& signer) {
    return {
        {"program_id", std::string(64, 'a')},
        {"accounts", json::array({{{"id", std::string(64, '1')}, {"signer", true}},
                                  {{"id", std::string(64, '2')}, {"signer", false}}})},
        {"instruction_words", json::array({1, 2})},
        {"signers", json::array({signer})},
        {"what", "transfer"},
        {"check", "dev balance --account <account>"},
    };
}

json tracking(const std::string& signer) {
    return {{"signer", signer},
            {"lock_path", "/state/locks/" + signer + ".lock"},
            {"nonce_before", "0"},
            {"height_before", 10}};
}

std::string sent(const std::string& hash) {
    return json{{"success", true}, {"tx_hash", hash}, {"secrets", json::array()}, {"error", ""}}.dump();
}

// Mocked C replies must outlive the call: the mock store keeps a pointer to the string.
const std::string& keep(std::string text) {
    static std::deque<std::string> store;
    store.push_back(std::move(text));
    return store.back();
}

struct Harness {
    LogosTestContext context{"auction_engine"};
    LogosModules modules{context.api()};
    AuctionEngineImpl module;

    Harness() {
        mock_ffi::reset();
        auction_engine::setClockForTests(&mock_ffi::fakeNowMs);
        module._logosCoreSetLogosModulesPtr_(&modules);
        ffi("auction_ffi_view",
            ok({{"network", "localnet"}, {"dev", true}, {"config", "cfg"}, {"genesis_ok", true}}));
        module.configure({{"deployment", "/d/deployment.json"},
                          {"state_dir", "/state"},
                          {"wallet_home", "/wallet"}});
    }

    ~Harness() { auction_engine::setClockForTests(nullptr); }

    void ffi(const std::string& name, const std::string& reply) {
        context.mockCFunction(name).returns(keep(reply));
    }

    void prepared(int steps) {
        json list = json::array();
        for (int i = 0; i < steps; ++i) list.push_back(step(i == 0 ? SIGNER_A : SIGNER_B));
        ffi("auction_ffi_prepare", ok({{"steps", list}, {"facts", json::array()}}));
        ffi("auction_ffi_tx_begin", ok(tracking(SIGNER_A)));
        ffi("auction_ffi_tx_end", ok(json::object()));
        context.mockModule("lez_core", SEND).returns(sent("ab"));
    }

    int calls(const std::string& ffiName) { return context.cFunctionCallCount(ffiName); }

    json lastRequest(const std::string& ffiName) {
        return json::parse(mock_ffi::state().requests[ffiName].back());
    }

    LogosMap submit() { return module.submit({{"action", "token_transfer"}}); }
};

}  // namespace

LOGOS_TEST(submit_sends_one_step_and_returns_the_hash) {
    Harness h;
    h.prepared(1);

    const LogosMap r = h.submit();

    LOGOS_ASSERT_EQ(r["hash"].get<std::string>(), std::string("ab"));
    LOGOS_ASSERT_EQ(r["what"].get<std::string>(), std::string("transfer"));
    LOGOS_ASSERT_EQ(r["step"].get<int>(), 0);
    LOGOS_ASSERT_EQ(r["steps"].get<int>(), 1);
    LOGOS_ASSERT_TRUE(r.contains("txId"));
    LOGOS_ASSERT_TRUE(r.contains("tracking"));
    LOGOS_ASSERT_EQ(h.context.moduleCallCount("lez_core", SEND), 1);
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 0);
    // The primary signer of step 0 takes the lock.
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_begin")["signer"].get<std::string>(), SIGNER_A);
}

LOGOS_TEST(a_failed_send_releases_the_lock) {
    Harness h;
    h.prepared(1);
    h.context.mockModule("lez_core", SEND)
        .returns(json{{"success", false},
                      {"tx_hash", ""},
                      {"secrets", json::array()},
                      {"error", "wallet FFI error 7"}}
                     .dump());

    const LogosMap r = h.submit();

    LOGOS_ASSERT_TRUE(r.contains("error"));
    LOGOS_ASSERT_CONTAINS(r["error"].get<std::string>(), "wallet FFI error 7");
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 1);
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_end")["outcome"].get<std::string>(),
                    std::string("dropped"));
}

LOGOS_TEST(tx_status_submits_the_next_step_after_inclusion) {
    Harness h;
    h.prepared(2);
    h.ffi("auction_ffi_tx_poll", ok({{"state", "included"}, {"nonce_after", "1"}}));
    const LogosMap first = h.submit();
    LOGOS_ASSERT_EQ(first["steps"].get<int>(), 2);

    const LogosMap r = h.module.txStatus({{"txId", first["txId"]}});

    LOGOS_ASSERT_EQ(r["state"].get<std::string>(), std::string("next"));
    LOGOS_ASSERT_EQ(r["step"].get<int>(), 1);
    LOGOS_ASSERT_EQ(h.context.moduleCallCount("lez_core", SEND), 2);
    // Step 0 is ended as included before step 1 takes its lock.
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 1);
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_end")["outcome"].get<std::string>(),
                    std::string("included"));
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_begin"), 2);
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_begin")["signer"].get<std::string>(), SIGNER_B);
}

LOGOS_TEST(tx_status_of_the_last_step_ends_the_lock_and_forgets_the_transaction) {
    Harness h;
    h.prepared(1);
    h.ffi("auction_ffi_tx_poll", ok({{"state", "included"}, {"nonce_after", "1"}}));
    const LogosMap first = h.submit();

    const LogosMap r = h.module.txStatus({{"txId", first["txId"]}});

    LOGOS_ASSERT_EQ(r["state"].get<std::string>(), std::string("included"));
    LOGOS_ASSERT_EQ(r["hash"].get<std::string>(), std::string("ab"));
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 1);
    LOGOS_ASSERT_TRUE(h.module.txStatus({{"txId", first["txId"]}}).contains("error"));
}

LOGOS_TEST(tx_status_reports_a_dropped_transaction_and_releases_the_lock) {
    Harness h;
    h.prepared(1);
    h.ffi("auction_ffi_tx_poll", ok({{"state", "dropped"}, {"height", 16}}));
    const LogosMap first = h.submit();

    const LogosMap r = h.module.txStatus({{"txId", first["txId"]}});

    LOGOS_ASSERT_EQ(r["state"].get<std::string>(), std::string("dropped"));
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_end")["outcome"].get<std::string>(),
                    std::string("dropped"));
}

LOGOS_TEST(tx_status_pending_keeps_the_lock_and_the_transaction) {
    Harness h;
    h.prepared(1);
    h.ffi("auction_ffi_tx_poll", ok({{"state", "pending"}, {"blocks_waited", 2}}));
    const LogosMap first = h.submit();

    const LogosMap r = h.module.txStatus({{"txId", first["txId"]}});

    LOGOS_ASSERT_EQ(r["state"].get<std::string>(), std::string("pending"));
    LOGOS_ASSERT_EQ(r["blocks_waited"].get<int>(), 2);
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 0);
    LOGOS_ASSERT_EQ(h.module.txStatus({{"txId", first["txId"]}})["state"].get<std::string>(),
                    std::string("pending"));
}

LOGOS_TEST(configure_returns_the_deployment_view) {
    Harness h;

    const json request = h.lastRequest("auction_ffi_view");
    LOGOS_ASSERT_EQ(request["view"].get<std::string>(), std::string("deployment"));
    LOGOS_ASSERT_EQ(request["ctx"]["state_dir"].get<std::string>(), std::string("/state"));

    h.ffi("auction_ffi_view",
          ok({{"network", "testnet-legacy"}, {"dev", false}, {"config", "cfg2"}, {"genesis_ok", true}}));
    const LogosMap r = h.module.configure({{"deployment", "/d/other.json"}, {"state_dir", "/state"}});
    LOGOS_ASSERT_EQ(r["network"].get<std::string>(), std::string("testnet-legacy"));
    LOGOS_ASSERT_EQ(r["dev"].get<bool>(), false);
    LOGOS_ASSERT_EQ(r["config"].get<std::string>(), std::string("cfg2"));
}

LOGOS_TEST(submit_before_configure_is_an_error) {
    LogosTestContext context("auction_engine");
    LogosModules modules(context.api());
    AuctionEngineImpl module;
    module._logosCoreSetLogosModulesPtr_(&modules);

    LOGOS_ASSERT_TRUE(module.submit({{"action", "token_transfer"}}).contains("error"));
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", SEND), 0);
}

LOGOS_TEST(open_wallet_treats_already_open_as_success) {
    Harness h;
    h.context.mockModule("lez_core", "open").returns(1);
    h.context.mockModule("lez_core", "get_current_block_height").returns(42);

    const LogosMap r = h.module.openWallet();

    LOGOS_ASSERT_EQ(r["opened"].get<bool>(), false);
    LOGOS_ASSERT_EQ(r["alreadyOpen"].get<bool>(), true);
    LOGOS_ASSERT_FALSE(r.contains("error"));
    LOGOS_ASSERT_TRUE(h.context.moduleCalledWith(
        "lez_core",
        "open",
        QVariantList{QVariant(QString("/wallet/wallet_config.json")),
                     QVariant(QString("/wallet/storage.json")),
                     QVariant(QString("/wallet/statistics.json"))}));
}

LOGOS_TEST(open_wallet_reports_success_when_it_opens) {
    Harness h;
    h.context.mockModule("lez_core", "open").returns(0);

    const LogosMap r = h.module.openWallet();

    LOGOS_ASSERT_EQ(r["opened"].get<bool>(), true);
    LOGOS_ASSERT_EQ(h.context.moduleCallCount("lez_core", "get_current_block_height"), 0);
}

LOGOS_TEST(open_wallet_reports_an_error_when_it_neither_opens_nor_is_open) {
    Harness h;
    h.context.mockModule("lez_core", "open").returns(1);
    h.context.mockModule("lez_core", "get_current_block_height").returns(0);

    LOGOS_ASSERT_TRUE(h.module.openWallet().contains("error"));
}

LOGOS_TEST(block_height_is_the_wallet_height) {
    Harness h;
    h.context.mockModule("lez_core", "get_current_block_height").returns(7);

    LOGOS_ASSERT_EQ(h.module.blockHeight()["height"].get<int>(), 7);

    h.context.mockModule("lez_core", "get_current_block_height").returns(0);
    LOGOS_ASSERT_TRUE(h.module.blockHeight().contains("error"));
}

// ---- R10: one 15 s budget across the FFI calls and the lez_core send of a module call ----

LOGOS_TEST(each_ffi_call_gets_what_is_left_of_the_module_budget) {
    Harness h;
    h.prepared(1);
    mock_ffi::state().costMs["auction_ffi_prepare"] = 4000;

    const LogosMap r = h.submit();

    LOGOS_ASSERT_TRUE(r.contains("hash"));
    // The FFI deadline stops reads from starting only (R13): the module leaves room for one read to run on.
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_prepare")["ctx"]["budget_ms"].get<std::int64_t>(),
                    MODULE_BUDGET_MS - VIEW_OVERRUN_MS);
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_begin")["ctx"]["budget_ms"].get<std::int64_t>(),
                    MODULE_BUDGET_MS - 4000 - TX_OVERRUN_MS);
}

LOGOS_TEST(no_ffi_call_starts_with_less_than_a_second_left) {
    Harness h;
    h.prepared(1);
    mock_ffi::state().costMs["auction_ffi_prepare"] = 14500;

    const LogosMap r = h.submit();

    LOGOS_ASSERT_TRUE(r.contains("error"));
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_begin"), 0);
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 0);
    LOGOS_ASSERT_EQ(h.context.moduleCallCount("lez_core", SEND), 0);
}

LOGOS_TEST(the_send_is_not_started_with_less_than_three_seconds_left_and_the_lock_is_released) {
    Harness h;
    h.prepared(1);
    mock_ffi::state().costMs["auction_ffi_tx_begin"] = 12500;

    const LogosMap r = h.submit();

    LOGOS_ASSERT_TRUE(r.contains("error"));
    LOGOS_ASSERT_EQ(r["kind"].get<std::string>(), std::string("budget"));
    LOGOS_ASSERT_EQ(h.context.moduleCallCount("lez_core", SEND), 0);
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 1);
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_end")["outcome"].get<std::string>(),
                    std::string("dropped"));
}

LOGOS_TEST(the_send_starts_with_exactly_three_seconds_left) {
    Harness h;
    h.prepared(1);
    mock_ffi::state().costMs["auction_ffi_tx_begin"] = 12000;

    LOGOS_ASSERT_TRUE(h.submit().contains("hash"));
    LOGOS_ASSERT_EQ(h.context.moduleCallCount("lez_core", SEND), 1);
}

LOGOS_TEST(tx_status_does_not_begin_the_next_step_when_the_budget_is_spent) {
    Harness h;
    h.prepared(2);
    h.ffi("auction_ffi_tx_poll", ok({{"state", "included"}, {"nonce_after", "1"}}));
    const LogosMap first = h.submit();
    // The status call has its own budget; the poll uses almost all of it.
    mock_ffi::state().costMs["auction_ffi_tx_poll"] = 14200;

    const LogosMap r = h.module.txStatus({{"txId", first["txId"]}});

    LOGOS_ASSERT_TRUE(r.contains("error"));
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_begin"), 1);
    LOGOS_ASSERT_EQ(h.context.moduleCallCount("lez_core", SEND), 1);
    // The included step was ended; no lock is held for a step that never began.
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 1);
}

LOGOS_TEST(a_new_module_call_starts_with_a_full_budget) {
    Harness h;
    h.prepared(1);
    mock_ffi::state().costMs["auction_ffi_prepare"] = 9000;
    LOGOS_ASSERT_TRUE(h.submit().contains("hash"));
    mock_ffi::state().costMs["auction_ffi_prepare"] = 0;

    LOGOS_ASSERT_TRUE(h.submit().contains("hash"));
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_prepare")["ctx"]["budget_ms"].get<std::int64_t>(),
                    MODULE_BUDGET_MS - VIEW_OVERRUN_MS);
}

LOGOS_TEST(a_budget_ms_in_the_configured_context_never_reaches_the_ffi) {
    Harness h;
    h.prepared(1);
    h.ffi("auction_ffi_view", ok({{"network", "localnet"}, {"dev", true}, {"config", "cfg"}}));
    h.module.configure({{"deployment", "/d/deployment.json"}, {"state_dir", "/state"}, {"budget_ms", 1}});

    LOGOS_ASSERT_TRUE(h.submit().contains("hash"));
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_prepare")["ctx"]["budget_ms"].get<std::int64_t>(),
                    MODULE_BUDGET_MS - VIEW_OVERRUN_MS);
}

// ---- review round 1 ----

LOGOS_TEST(a_failed_release_note_survives_the_next_step) {
    Harness h;
    h.prepared(2);
    h.ffi("auction_ffi_tx_poll", ok({{"state", "included"}, {"nonce_after", "1"}}));
    const LogosMap first = h.submit();
    h.ffi("auction_ffi_tx_end",
          json{{"ok", false}, {"error", {{"kind", "bad_request"}, {"message", "lock file busy"}}}}.dump());

    const LogosMap r = h.module.txStatus({{"txId", first["txId"]}});

    LOGOS_ASSERT_EQ(r["state"].get<std::string>(), std::string("next"));
    LOGOS_ASSERT_CONTAINS(r["note"].get<std::string>(), "lock file busy");
}

LOGOS_TEST(a_failed_release_note_survives_a_failed_next_step) {
    Harness h;
    h.prepared(2);
    h.ffi("auction_ffi_tx_poll", ok({{"state", "included"}, {"nonce_after", "1"}}));
    const LogosMap first = h.submit();
    h.ffi("auction_ffi_tx_end",
          json{{"ok", false}, {"error", {{"kind", "bad_request"}, {"message", "lock file busy"}}}}.dump());
    h.context.mockModule("lez_core", SEND)
        .returns(json{{"success", false}, {"tx_hash", ""}, {"error", "refused"}}.dump());

    const LogosMap r = h.module.txStatus({{"txId", first["txId"]}});

    LOGOS_ASSERT_TRUE(r.contains("error"));
    LOGOS_ASSERT_CONTAINS(r["note"].get<std::string>(), "lock file busy");
}

LOGOS_TEST(a_mistyped_poll_reply_is_an_error_not_an_exception_and_keeps_the_transaction) {
    Harness h;
    h.prepared(1);
    h.ffi("auction_ffi_tx_poll", ok({{"state", "pending"}, {"blocks_waited", "two"}}));
    const LogosMap first = h.submit();

    const LogosMap r = h.module.txStatus({{"txId", first["txId"]}});

    LOGOS_ASSERT_EQ(r["kind"].get<std::string>(), std::string("bad_reply"));
    h.ffi("auction_ffi_tx_poll", ok({{"state", "pending"}, {"blocks_waited", 3}}));
    LOGOS_ASSERT_EQ(h.module.txStatus({{"txId", first["txId"]}})["state"].get<std::string>(),
                    std::string("pending"));
}

LOGOS_TEST(a_reply_whose_value_is_not_an_object_is_a_bad_reply) {
    Harness h;
    h.prepared(1);
    h.ffi("auction_ffi_prepare", json{{"ok", true}, {"value", json::array()}}.dump());

    LOGOS_ASSERT_EQ(h.submit()["kind"].get<std::string>(), std::string("bad_reply"));
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_begin"), 0);
}

LOGOS_TEST(a_mistyped_view_reply_is_a_bad_reply) {
    Harness h;
    h.ffi("auction_ffi_view", ok({{"network", "localnet"}, {"dev", "yes"}}));

    const LogosMap r = h.module.configure({{"deployment", "/d.json"}, {"state_dir", "/s"}});

    LOGOS_ASSERT_EQ(r["kind"].get<std::string>(), std::string("bad_reply"));
}

LOGOS_TEST(an_ffi_error_without_kind_or_message_still_has_both) {
    Harness h;
    h.prepared(1);
    h.ffi("auction_ffi_prepare", json{{"ok", false}, {"error", json::object()}}.dump());

    const LogosMap r = h.submit();

    LOGOS_ASSERT_FALSE(r["error"].get<std::string>().empty());
    LOGOS_ASSERT_FALSE(r["kind"].get<std::string>().empty());
}

LOGOS_TEST(a_bad_plan_step_is_refused_before_the_lock_is_taken) {
    Harness h;
    h.prepared(1);
    json bad = step(SIGNER_A);
    bad["instruction_words"] = json::array({4294967296LL});
    h.ffi("auction_ffi_prepare", ok({{"steps", json::array({bad})}, {"facts", json::array()}}));

    const LogosMap r = h.submit();

    LOGOS_ASSERT_EQ(r["kind"].get<std::string>(), std::string("bad_plan"));
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_begin"), 0);
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 0);
}

LOGOS_TEST(a_send_without_a_hash_is_an_unknown_outcome_not_a_hash) {
    Harness h;
    h.prepared(1);
    h.context.mockModule("lez_core", SEND)
        .returns(json{{"success", true}, {"tx_hash", ""}, {"error", ""}}.dump());

    const LogosMap r = h.submit();

    LOGOS_ASSERT_TRUE(r.contains("error"));
    LOGOS_ASSERT_FALSE(r.contains("hash"));
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 1);
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_end")["outcome"].get<std::string>(),
                    std::string("unknown"));
}

LOGOS_TEST(a_transport_failure_of_the_send_keeps_the_lock_as_unknown) {
    Harness h;
    h.prepared(1);
    lez_test_hooks::failNextSend = true;

    const LogosMap r = h.submit();

    LOGOS_ASSERT_EQ(r["kind"].get<std::string>(), std::string("send"));
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 1);
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_end")["outcome"].get<std::string>(),
                    std::string("unknown"));
}

LOGOS_TEST(open_wallet_sees_an_open_wallet_by_its_accounts) {
    Harness h;
    h.context.mockModule("lez_core", "open").returns(1);
    h.context.mockModule("lez_core", "get_current_block_height").returns(0);
    h.context.mockModule("lez_core", "list_accounts")
        .returnsVariant(QVariant(QVariantList{
            QVariant(QVariantMap{{"account_id", "aa"}, {"is_public", true}})}));

    const LogosMap r = h.module.openWallet();

    LOGOS_ASSERT_EQ(r["alreadyOpen"].get<bool>(), true);
}
