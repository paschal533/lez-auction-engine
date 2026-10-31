#include <string>

#include <logos_test.h>
#include <nlohmann/json.hpp>

#include "harness.h"

using namespace harness;

namespace {

const std::string ACCOUNT_OWNER = "8y1Fz1QvQmTq2o2Wn6r8bZyJxGkTnQpC7bL4dS3aHk9";
const std::string ACCOUNT_TAKER = "3QmN7pWv5cL9rT2yK8dF4hJ6sB1xZ0aE7gU3iO5nP2q";

/// The context the module adds to every FFI call: the configured one plus the view budget (R13, R54).
json viewCtx() {
    json ctx = configuredCtx();
    ctx["budget_ms"] = VIEW_BUDGET_MS;
    return ctx;
}

json accountsRows() {
    return json::array({
        {{"label", "owner"},
         {"id", ACCOUNT_OWNER},
         {"role", "owner"},
         {"stable_balance", "1000000"},
         {"coll_balance", nullptr},
         {"holding", "owner-stable"},
         {"extra", "not for the picker"}},
        {{"label", "taker"},
         {"id", ACCOUNT_TAKER},
         {"role", "taker"},
         {"stable_balance", nullptr},
         {"coll_balance", "340282366920938463463374607431768211455"},
         {"holding", "taker-coll"}},
    });
}

}  // namespace

LOGOS_TEST(view_passes_ctx_and_request) {
    Harness h;
    h.ffi("auction_ffi_view", ok({{"block", 3}, {"counts", {{"live", 1}}}}));

    // A caller's ctx never reaches auction_ffi: the configured one replaces it.
    const LogosMap r = h.module.view(
        {{"view", "auction"}, {"id", ACCOUNT_OWNER}, {"ctx", {{"deployment", "/elsewhere.json"}}}});

    LOGOS_ASSERT_EQ(r["block"].get<int>(), 3);
    LOGOS_ASSERT_EQ(r["counts"]["live"].get<int>(), 1);
    LOGOS_ASSERT_TRUE(h.cFunctionCalledWith(
        "auction_ffi_view", {{"view", "auction"}, {"id", ACCOUNT_OWNER}, {"ctx", viewCtx()}}));
}

LOGOS_TEST(view_passes_an_ffi_error_through) {
    Harness h;
    h.ffi("auction_ffi_view", failed("refused", "no surplus auction was started"));

    const LogosMap r = h.module.view({{"view", "quote_take_surplus"}, {"amount", "1"}});

    LOGOS_ASSERT_EQ(r["kind"].get<std::string>(), std::string("refused"));
    LOGOS_ASSERT_CONTAINS(r["error"].get<std::string>(), "no surplus auction");
}

LOGOS_TEST(view_needs_a_view_name_and_a_configured_module) {
    Harness h;
    const int before = h.calls("auction_ffi_view");
    LOGOS_ASSERT_EQ(h.module.view({{"id", "x"}})["kind"].get<std::string>(), std::string("bad_request"));
    LOGOS_ASSERT_EQ(h.module.view(json::array())["kind"].get<std::string>(), std::string("bad_request"));
    LOGOS_ASSERT_EQ(h.module.view({{"view", "accounts"}})["kind"].get<std::string>(), std::string("bad_request"));
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_view"), before);

    AuctionEngineImpl fresh;
    LOGOS_ASSERT_EQ(fresh.view({{"view", "overview"}})["kind"].get<std::string>(),
                    std::string("not_configured"));
}

LOGOS_TEST(accounts_maps_the_view) {
    Harness h;
    h.ffi("auction_ffi_view", ok(accountsRows()));

    const LogosList rows = h.module.accounts();

    LOGOS_ASSERT_TRUE(rows.is_array());
    LOGOS_ASSERT_EQ(static_cast<int>(rows.size()), 2);
    const json want0 = {{"label", "owner"},
                        {"id", ACCOUNT_OWNER},
                        {"role", "owner"},
                        {"stable_balance", "1000000"},
                        {"coll_balance", nullptr},
                        {"holding", "owner-stable"}};
    LOGOS_ASSERT_EQ(rows[0], want0);
    // u128 balances stay decimal strings, all digits kept.
    LOGOS_ASSERT_EQ(rows[1]["coll_balance"].get<std::string>(),
                    std::string("340282366920938463463374607431768211455"));
    LOGOS_ASSERT_TRUE(rows[1]["stable_balance"].is_null());
    LOGOS_ASSERT_TRUE(h.cFunctionCalledWith("auction_ffi_view", {{"view", "accounts"}, {"ctx", viewCtx()}}));
}

LOGOS_TEST(accounts_reports_an_error_as_its_only_row) {
    Harness h;
    h.ffi("auction_ffi_view", failed("keystore", "storage.json not found"));

    const LogosList rows = h.module.accounts();

    LOGOS_ASSERT_EQ(static_cast<int>(rows.size()), 1);
    LOGOS_ASSERT_EQ(rows[0]["kind"].get<std::string>(), std::string("keystore"));
    LOGOS_ASSERT_CONTAINS(rows[0]["error"].get<std::string>(), "storage.json");

    // A reply that is not a list of rows is a bad reply, not an exception.
    h.ffi("auction_ffi_view", ok({{"label", "owner"}}));
    LOGOS_ASSERT_EQ(h.module.accounts()[0]["kind"].get<std::string>(), std::string("bad_reply"));
    h.ffi("auction_ffi_view", ok(json::array({"owner"})));
    LOGOS_ASSERT_EQ(h.module.accounts()[0]["kind"].get<std::string>(), std::string("bad_reply"));
}

LOGOS_TEST(rescan_is_one_page) {
    Harness h;
    const json page = {{"scanned_to", 100}, {"head", 250}, {"done", false}, {"restarted", false}, {"note", nullptr}};
    h.ffi("auction_ffi_view", ok(page));
    const int before = h.calls("auction_ffi_view");

    const LogosMap r = h.module.rescan(json::object());

    // One FFI call, one page: the UI calls again until `done`.
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_view") - before, 1);
    LOGOS_ASSERT_EQ(r, page);
    LOGOS_ASSERT_TRUE(h.cFunctionCalledWith(
        "auction_ffi_view", {{"view", "scan"}, {"max_blocks", 100}, {"ctx", viewCtx()}}));

    h.module.rescan({{"max_blocks", 20}});
    LOGOS_ASSERT_EQ(h.calls("auction_ffi_view") - before, 2);
    LOGOS_ASSERT_TRUE(h.cFunctionCalledWith(
        "auction_ffi_view", {{"view", "scan"}, {"max_blocks", 20}, {"ctx", viewCtx()}}));
}

LOGOS_TEST(rescan_reports_a_restarted_chain) {
    Harness h;
    h.ffi("auction_ffi_view",
          ok({{"scanned_to", 4}, {"head", 4}, {"done", true}, {"restarted", true}, {"note", "chain replaced"}}));

    const LogosMap r = h.module.rescan(json::object());

    LOGOS_ASSERT_EQ(r["restarted"].get<bool>(), true);
    LOGOS_ASSERT_EQ(r["done"].get<bool>(), true);
}
