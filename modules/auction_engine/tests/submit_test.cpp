#include <string>

#include <QVariant>
#include <QVariantList>
#include <logos_test.h>
#include <nlohmann/json.hpp>

#include "harness.h"

using namespace harness;

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

namespace {

// R53: lez_core's success:false may come after the request reached the sequencer (a proxy 502, a response
// timeout), so it is never reported as dropped and the lock is not released.
LogosMap failedSend(Harness& h) {
    h.context.mockModule("lez_core", SEND)
        .returns(json{{"success", false},
                      {"tx_hash", ""},
                      {"secrets", json::array()},
                      {"error", "wallet FFI error 7"}}
                     .dump());
    return h.submit();
}

}  // namespace

LOGOS_TEST(a_failed_send_reports_lez_cores_error_and_does_not_release_the_lock) {
    Harness h;
    h.prepared(1);

    const LogosMap r = failedSend(h);

    LOGOS_ASSERT_TRUE(r.contains("error"));
    LOGOS_ASSERT_CONTAINS(r["error"].get<std::string>(), "wallet FFI error 7");
    LOGOS_ASSERT_CONTAINS(r["error"].get<std::string>(), "may or may not have reached the node");
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 1);
    LOGOS_ASSERT_TRUE(h.lastRequest("auction_ffi_tx_end")["outcome"].get<std::string>() != "dropped");
}

LOGOS_TEST(a_failed_send_keeps_the_lock_as_unknown) {
    Harness h;
    h.prepared(1);

    const LogosMap r = failedSend(h);

    LOGOS_ASSERT_EQ(r["kind"].get<std::string>(), std::string("send"));
    LOGOS_ASSERT_EQ(r["outcome"].get<std::string>(), std::string("unknown"));
    LOGOS_ASSERT_EQ(r["step"].get<int>(), 0);
    LOGOS_ASSERT_EQ(r["includedSteps"].get<int>(), 0);
    LOGOS_ASSERT_FALSE(r.contains("txId"));
    // tx_end "unknown" keeps the lock file for the staleness rule (nonce moved, or drop_after_blocks).
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_end")["outcome"].get<std::string>(), std::string("unknown"));
    LOGOS_ASSERT_EQ(h.context.moduleCallCount("lez_core", SEND), 1);
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

    // No height is the node not answering (the app's heartbeat shows "Node not answering" on `chain`).
    h.context.mockModule("lez_core", "get_current_block_height").returns(0);
    const LogosMap r = h.module.blockHeight();
    LOGOS_ASSERT_TRUE(r.contains("error"));
    LOGOS_ASSERT_EQ(r["kind"].get<std::string>(), std::string("chain"));
}

// ---- R10: one 15 s budget across the FFI calls and the lez_core send of a module call ----

LOGOS_TEST(each_ffi_call_gets_what_is_left_of_the_module_budget) {
    Harness h;
    h.prepared(1);
    mock_ffi::state().costMs["auction_ffi_prepare"] = 4000;

    const LogosMap r = h.submit();

    LOGOS_ASSERT_TRUE(r.contains("hash"));
    // auction_ffi cuts its reads off at the deadline (R54): the module keeps time only for what follows.
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_prepare")["ctx"]["budget_ms"].get<std::int64_t>(), PREPARE_BUDGET_MS);
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_begin")["ctx"]["budget_ms"].get<std::int64_t>(),
                    MODULE_BUDGET_MS - 4000 - TX_BEGIN_KEEP_MS);
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
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_prepare")["ctx"]["budget_ms"].get<std::int64_t>(), PREPARE_BUDGET_MS);
}

LOGOS_TEST(a_budget_ms_in_the_configured_context_never_reaches_the_ffi) {
    Harness h;
    h.prepared(1);
    h.ffi("auction_ffi_view", ok({{"network", "localnet"}, {"dev", true}, {"config", "cfg"}}));
    h.module.configure({{"deployment", "/d/deployment.json"}, {"state_dir", "/state"}, {"budget_ms", 1}});

    LOGOS_ASSERT_TRUE(h.submit().contains("hash"));
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_prepare")["ctx"]["budget_ms"].get<std::int64_t>(), PREPARE_BUDGET_MS);
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

// ---- multi-step sequences: each step its own tracking, one transaction in flight per signer ----

namespace {

// oracle_publish: three steps, three signers (the plan Task 9's prepare returns).
json oraclePublishSteps() {
    return json::array({step(SIGNER_A, "publish price (source A)"),
                        step(SIGNER_B, "publish price (source B)"),
                        step(SIGNER_C, "publish price (source C)")});
}

}  // namespace

LOGOS_TEST(oracle_publish_runs_three_steps_in_order) {
    Harness h;
    h.preparedSteps(oraclePublishSteps());
    h.ffi("auction_ffi_tx_poll", ok({{"state", "included"}, {"nonce_after", "1"}}));
    const LogosMap first = h.module.submit({{"action", "oracle_publish"}, {"price", "1000"}});
    LOGOS_ASSERT_EQ(first["steps"].get<int>(), 3);
    LOGOS_ASSERT_EQ(first["tracking"]["signer"].get<std::string>(), SIGNER_A);

    h.ffi("auction_ffi_tx_begin", ok(tracking(SIGNER_B)));
    const LogosMap second = h.module.txStatus({{"txId", first["txId"]}});
    LOGOS_ASSERT_EQ(second["state"].get<std::string>(), std::string("next"));
    LOGOS_ASSERT_EQ(second["step"].get<int>(), 1);
    LOGOS_ASSERT_EQ(second["tracking"]["signer"].get<std::string>(), SIGNER_B);

    h.ffi("auction_ffi_tx_begin", ok(tracking(SIGNER_C)));
    const LogosMap third = h.module.txStatus({{"txId", first["txId"]}});
    LOGOS_ASSERT_EQ(third["state"].get<std::string>(), std::string("next"));
    LOGOS_ASSERT_EQ(third["step"].get<int>(), 2);
    LOGOS_ASSERT_EQ(third["tracking"]["signer"].get<std::string>(), SIGNER_C);

    const LogosMap done = h.module.txStatus({{"txId", first["txId"]}});
    LOGOS_ASSERT_EQ(done["state"].get<std::string>(), std::string("included"));
    LOGOS_ASSERT_EQ(done["step"].get<int>(), 2);

    // One lock per step, taken by that step's signer, in plan order; each released before the next is taken.
    const std::vector<json> begun = h.requests("auction_ffi_tx_begin");
    LOGOS_ASSERT_EQ(static_cast<int>(begun.size()), 3);
    LOGOS_ASSERT_EQ(begun[0]["signer"].get<std::string>(), SIGNER_A);
    LOGOS_ASSERT_EQ(begun[1]["signer"].get<std::string>(), SIGNER_B);
    LOGOS_ASSERT_EQ(begun[2]["signer"].get<std::string>(), SIGNER_C);
    const std::vector<json> ended = h.requests("auction_ffi_tx_end");
    LOGOS_ASSERT_EQ(static_cast<int>(ended.size()), 3);
    LOGOS_ASSERT_EQ(ended[0]["tracking"]["signer"].get<std::string>(), SIGNER_A);
    LOGOS_ASSERT_EQ(ended[1]["tracking"]["signer"].get<std::string>(), SIGNER_B);
    LOGOS_ASSERT_EQ(ended[2]["tracking"]["signer"].get<std::string>(), SIGNER_C);
    for (const json& end : ended) {
        LOGOS_ASSERT_EQ(end["outcome"].get<std::string>(), std::string("included"));
    }
    LOGOS_ASSERT_EQ(h.context.moduleCallCount("lez_core", SEND), 3);
    LOGOS_ASSERT_EQ(h.module.txStatus({{"txId", first["txId"]}})["kind"].get<std::string>(),
                    std::string("unknown_tx"));
}

LOGOS_TEST(a_dropped_middle_step_stops_the_sequence) {
    Harness h;
    h.preparedSteps(oraclePublishSteps());
    h.ffi("auction_ffi_tx_poll", ok({{"state", "included"}, {"nonce_after", "1"}}));
    const LogosMap first = h.module.submit({{"action", "oracle_publish"}, {"price", "1000"}});
    h.ffi("auction_ffi_tx_begin", ok(tracking(SIGNER_B)));
    LOGOS_ASSERT_EQ(h.module.txStatus({{"txId", first["txId"]}})["state"].get<std::string>(),
                    std::string("next"));

    h.ffi("auction_ffi_tx_poll", ok({{"state", "dropped"}, {"height", 20}}));
    const LogosMap r = h.module.txStatus({{"txId", first["txId"]}});

    LOGOS_ASSERT_EQ(r["state"].get<std::string>(), std::string("dropped"));
    LOGOS_ASSERT_EQ(r["step"].get<int>(), 1);
    LOGOS_ASSERT_EQ(r["includedSteps"].get<int>(), 1);
    LOGOS_ASSERT_EQ(r["discardedSteps"].get<int>(), 1);
    // Step 2 never began: two locks taken, two released (included, then dropped), two sends.
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_begin"), 2);
    const std::vector<json> ended = h.requests("auction_ffi_tx_end");
    LOGOS_ASSERT_EQ(static_cast<int>(ended.size()), 2);
    LOGOS_ASSERT_EQ(ended[0]["outcome"].get<std::string>(), std::string("included"));
    LOGOS_ASSERT_EQ(ended[1]["outcome"].get<std::string>(), std::string("dropped"));
    LOGOS_ASSERT_EQ(ended[1]["tracking"]["signer"].get<std::string>(), SIGNER_B);
    LOGOS_ASSERT_EQ(h.context.moduleCallCount("lez_core", SEND), 2);
    // The remaining step is discarded with the sequence.
    LOGOS_ASSERT_EQ(h.module.txStatus({{"txId", first["txId"]}})["kind"].get<std::string>(),
                    std::string("unknown_tx"));
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_begin"), 2);
}

LOGOS_TEST(a_second_send_by_a_signer_in_flight_is_refused_and_another_signer_may_send) {
    Harness h;
    h.prepared(1);
    const LogosMap first = h.submit();
    LOGOS_ASSERT_TRUE(first.contains("hash"));

    // auction_ffi holds SIGNER_A's lock: its tx_begin answers in_flight, and nothing is sent.
    h.ffi("auction_ffi_tx_begin", failed("in_flight", "SignerA has a transaction in flight"));
    const LogosMap refused = h.submit();
    LOGOS_ASSERT_EQ(refused["kind"].get<std::string>(), std::string("in_flight"));
    LOGOS_ASSERT_EQ(h.context.moduleCallCount("lez_core", SEND), 1);
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 0);

    // Another signer's plan takes its own lock and is sent while the first is still pending.
    h.preparedSteps(json::array({step(SIGNER_B)}));
    const LogosMap other = h.submit();
    LOGOS_ASSERT_TRUE(other.contains("hash"));
    LOGOS_ASSERT_TRUE(other["txId"] != first["txId"]);
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_begin")["signer"].get<std::string>(), SIGNER_B);
    LOGOS_ASSERT_EQ(h.context.moduleCallCount("lez_core", SEND), 2);
}

LOGOS_TEST(every_sequence_end_reports_included_and_discarded_steps_the_same_way) {
    // Begin failure of step 0 (submit): nothing on chain, the two later steps discarded.
    {
        Harness h;
        h.preparedSteps(oraclePublishSteps());
        h.ffi("auction_ffi_tx_begin", failed("in_flight", "SignerA has a transaction in flight"));
        const LogosMap r = h.module.submit({{"action", "oracle_publish"}, {"price", "1000"}});
        LOGOS_ASSERT_EQ(r["kind"].get<std::string>(), std::string("in_flight"));
        LOGOS_ASSERT_EQ(r["outcome"].get<std::string>(), std::string("not_sent"));
        LOGOS_ASSERT_EQ(r["step"].get<int>(), 0);
        LOGOS_ASSERT_EQ(r["includedSteps"].get<int>(), 0);
        LOGOS_ASSERT_EQ(r["discardedSteps"].get<int>(), 2);
        // No lock was taken, so none is ended.
        LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 0);
    }
    // Send failure of step 0 (submit).
    {
        Harness h;
        h.preparedSteps(oraclePublishSteps());
        h.context.mockModule("lez_core", SEND)
            .returns(json{{"success", false}, {"tx_hash", ""}, {"error", "refused"}}.dump());
        const LogosMap r = h.module.submit({{"action", "oracle_publish"}, {"price", "1000"}});
        LOGOS_ASSERT_EQ(r["kind"].get<std::string>(), std::string("send"));
        LOGOS_ASSERT_EQ(r["outcome"].get<std::string>(), std::string("unknown"));
        LOGOS_ASSERT_EQ(r["step"].get<int>(), 0);
        LOGOS_ASSERT_EQ(r["includedSteps"].get<int>(), 0);
        LOGOS_ASSERT_EQ(r["discardedSteps"].get<int>(), 2);
        LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 1);
        LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_end")["outcome"].get<std::string>(), std::string("unknown"));
    }
    // Begin failure of step 1 (txStatus after step 0 is included): one on chain, one discarded.
    {
        Harness h;
        h.preparedSteps(oraclePublishSteps());
        h.ffi("auction_ffi_tx_poll", ok({{"state", "included"}, {"nonce_after", "1"}}));
        const LogosMap first = h.module.submit({{"action", "oracle_publish"}, {"price", "1000"}});
        h.ffi("auction_ffi_tx_begin", failed("in_flight", "SignerB has a transaction in flight"));
        const LogosMap r = h.module.txStatus({{"txId", first["txId"]}});
        LOGOS_ASSERT_EQ(r["kind"].get<std::string>(), std::string("in_flight"));
        LOGOS_ASSERT_EQ(r["outcome"].get<std::string>(), std::string("not_sent"));
        LOGOS_ASSERT_EQ(r["step"].get<int>(), 1);
        LOGOS_ASSERT_EQ(r["includedSteps"].get<int>(), 1);
        LOGOS_ASSERT_EQ(r["discardedSteps"].get<int>(), 1);
        LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 1);
        LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_end")["outcome"].get<std::string>(), std::string("included"));
    }
    // Send failure of step 1 (txStatus).
    {
        Harness h;
        h.preparedSteps(oraclePublishSteps());
        h.ffi("auction_ffi_tx_poll", ok({{"state", "included"}, {"nonce_after", "1"}}));
        const LogosMap first = h.module.submit({{"action", "oracle_publish"}, {"price", "1000"}});
        h.ffi("auction_ffi_tx_begin", ok(tracking(SIGNER_B)));
        h.context.mockModule("lez_core", SEND)
            .returns(json{{"success", false}, {"tx_hash", ""}, {"error", "refused"}}.dump());
        const LogosMap r = h.module.txStatus({{"txId", first["txId"]}});
        LOGOS_ASSERT_EQ(r["kind"].get<std::string>(), std::string("send"));
        LOGOS_ASSERT_EQ(r["outcome"].get<std::string>(), std::string("unknown"));
        LOGOS_ASSERT_EQ(r["step"].get<int>(), 1);
        LOGOS_ASSERT_EQ(r["includedSteps"].get<int>(), 1);
        LOGOS_ASSERT_EQ(r["discardedSteps"].get<int>(), 1);
        const std::vector<json> ended = h.requests("auction_ffi_tx_end");
        LOGOS_ASSERT_EQ(static_cast<int>(ended.size()), 2);
        LOGOS_ASSERT_EQ(ended[0]["outcome"].get<std::string>(), std::string("included"));
        LOGOS_ASSERT_EQ(ended[1]["outcome"].get<std::string>(), std::string("unknown"));
    }
    // A send of step 1 whose fate is not known (unreadable lez_core reply): unknown, the lock is kept.
    {
        Harness h;
        h.preparedSteps(oraclePublishSteps());
        h.ffi("auction_ffi_tx_poll", ok({{"state", "included"}, {"nonce_after", "1"}}));
        const LogosMap first = h.module.submit({{"action", "oracle_publish"}, {"price", "1000"}});
        h.ffi("auction_ffi_tx_begin", ok(tracking(SIGNER_B)));
        h.context.mockModule("lez_core", SEND).returns(std::string("not json"));
        const LogosMap r = h.module.txStatus({{"txId", first["txId"]}});
        LOGOS_ASSERT_EQ(r["kind"].get<std::string>(), std::string("send"));
        LOGOS_ASSERT_EQ(r["outcome"].get<std::string>(), std::string("unknown"));
        LOGOS_ASSERT_EQ(r["step"].get<int>(), 1);
        LOGOS_ASSERT_EQ(r["includedSteps"].get<int>(), 1);
        LOGOS_ASSERT_EQ(r["discardedSteps"].get<int>(), 1);
        LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_end")["outcome"].get<std::string>(), std::string("unknown"));
    }
    // A dropped step (tx_poll) ends the sequence with outcome dropped too.
    {
        Harness h;
        h.prepared(1);
        h.ffi("auction_ffi_tx_poll", ok({{"state", "dropped"}, {"height", 16}}));
        const LogosMap first = h.submit();
        const LogosMap r = h.module.txStatus({{"txId", first["txId"]}});
        LOGOS_ASSERT_EQ(r["outcome"].get<std::string>(), std::string("dropped"));
        LOGOS_ASSERT_EQ(r["includedSteps"].get<int>(), 0);
    }
}

LOGOS_TEST(tx_status_echoes_drop_after_blocks_default_five) {
    Harness h;
    h.prepared(1);
    h.ffi("auction_ffi_tx_poll", ok({{"state", "pending"}, {"blocks_waited", 1}}));
    const LogosMap first = h.submit();
    LOGOS_ASSERT_EQ(h.module.txStatus({{"txId", first["txId"]}})["drop_after_blocks"].get<int>(), 5);
    h.ffi("auction_ffi_tx_poll", ok({{"state", "dropped"}, {"height", 16}}));
    LOGOS_ASSERT_EQ(h.module.txStatus({{"txId", first["txId"]}})["drop_after_blocks"].get<int>(), 5);
}

LOGOS_TEST(tx_status_echoes_drop_after_blocks_from_the_ctx) {
    Harness h;
    h.module.configure({{"deployment", "/d/deployment.json"}, {"state_dir", "/state"}, {"drop_after_blocks", 8}});
    h.prepared(1);
    h.ffi("auction_ffi_tx_poll", ok({{"state", "pending"}, {"blocks_waited", 1}}));
    const LogosMap first = h.submit();
    LOGOS_ASSERT_EQ(h.module.txStatus({{"txId", first["txId"]}})["drop_after_blocks"].get<int>(), 8);
}

// ---- final review ----

LOGOS_TEST(a_chain_error_from_tx_poll_keeps_the_transaction_and_its_lock) {
    Harness h;
    h.prepared(1);
    const LogosMap first = h.submit();
    h.ffi("auction_ffi_tx_poll", failed("chain", "getAccountsNonces: request timeout"));

    const LogosMap r = h.module.txStatus({{"txId", first["txId"]}});

    LOGOS_ASSERT_EQ(r["kind"].get<std::string>(), std::string("chain"));
    LOGOS_ASSERT_FALSE(r.contains("outcome"));
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_tx_end"), 0);
    // The entry is kept: the next poll reads the chain again.
    h.ffi("auction_ffi_tx_poll", ok({{"state", "pending"}, {"blocks_waited", 1}}));
    LOGOS_ASSERT_EQ(h.module.txStatus({{"txId", first["txId"]}})["state"].get<std::string>(),
                    std::string("pending"));
}

LOGOS_TEST(a_transaction_in_flight_is_polled_under_the_ctx_it_was_sent_with) {
    Harness h;
    h.prepared(2);
    const LogosMap first = h.submit();
    h.ffi("auction_ffi_view", ok({{"network", "testnet-legacy"}, {"dev", false}, {"config", "cfg2"}}));
    h.module.configure({{"deployment", "/d/other.json"}, {"state_dir", "/other-state"}});
    h.ffi("auction_ffi_tx_poll", ok({{"state", "included"}, {"nonce_after", "1"}}));

    LOGOS_ASSERT_EQ(h.module.txStatus({{"txId", first["txId"]}})["state"].get<std::string>(), std::string("next"));

    for (const char* name : {"auction_ffi_tx_poll", "auction_ffi_tx_end", "auction_ffi_tx_begin"}) {
        const json ctx = h.lastRequest(name)["ctx"];
        LOGOS_ASSERT_EQ(ctx["deployment"].get<std::string>(), std::string("/d/deployment.json"));
        LOGOS_ASSERT_EQ(ctx["state_dir"].get<std::string>(), std::string("/state"));
    }
    // A new submit plans under the new ctx.
    h.submit();
    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_prepare")["ctx"]["deployment"].get<std::string>(),
                    std::string("/d/other.json"));
}

// R54: a view's reads share one ~14 s deadline (the old 4 s failed overview and live on a real node).
LOGOS_TEST(a_view_gets_about_fourteen_seconds_for_all_its_reads) {
    Harness h;
    h.ffi("auction_ffi_view", ok({{"block", 1}}));

    h.module.view({{"view", "overview"}});

    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_view")["ctx"]["budget_ms"].get<std::int64_t>(), VIEW_BUDGET_MS);
}

LOGOS_TEST(tx_status_polls_with_what_is_left_less_a_begin_and_send) {
    Harness h;
    h.prepared(1);
    h.ffi("auction_ffi_tx_poll", ok({{"state", "pending"}, {"blocks_waited", 1}}));
    const LogosMap first = h.submit();

    h.module.txStatus({{"txId", first["txId"]}});

    LOGOS_ASSERT_EQ(h.lastRequest("auction_ffi_tx_poll")["ctx"]["budget_ms"].get<std::int64_t>(), PREPARE_BUDGET_MS);
}
