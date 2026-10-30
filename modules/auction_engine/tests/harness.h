#pragma once

#include <cstdint>
#include <deque>
#include <string>
#include <vector>

#include <logos_test.h>
#include <nlohmann/json.hpp>

#include "auction_engine_impl.h"
#include "budget.h"
#include "logos_sdk.h"
#include "mock_ffi_state.h"

// Shared test fixture: a configured module over the mocked auction_ffi and a mocked lez_core.
namespace harness {

using json = nlohmann::json;

inline const std::string SIGNER_A = "SignerAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
inline const std::string SIGNER_B = "SignerBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
inline const std::string SIGNER_C = "SignerCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC";
inline const std::string SEND = "send_generic_public_transaction";
constexpr std::int64_t MODULE_BUDGET_MS = 15000;
// What each call kind is handed with the whole budget left (Ruling R54): auction_ffi cuts its reads off at the
// deadline, so the module keeps only time for the reply (1 s) and, before a tx_begin and send, for those (6 s).
constexpr std::int64_t VIEW_BUDGET_MS = 14000;
constexpr std::int64_t PREPARE_BUDGET_MS = 8000;  // also tx_poll: a tx_begin and a send may follow
constexpr std::int64_t TX_BEGIN_KEEP_MS = 4000;   // the reply and the 3 s the send needs to start

inline std::string ok(const json& value) {
    return json{{"ok", true}, {"value", value}}.dump();
}

inline std::string failed(const std::string& kind, const std::string& message) {
    return json{{"ok", false}, {"error", {{"kind", kind}, {"message", message}}}}.dump();
}

inline json step(const std::string& signer, const std::string& what = "transfer") {
    return {
        {"program_id", std::string(64, 'a')},
        {"accounts", json::array({{{"id", std::string(64, '1')}, {"signer", true}},
                                  {{"id", std::string(64, '2')}, {"signer", false}}})},
        {"instruction_words", json::array({1, 2})},
        {"signers", json::array({signer})},
        {"what", what},
        {"check", "dev balance --account <account>"},
    };
}

inline json tracking(const std::string& signer) {
    return {{"signer", signer},
            {"lock_path", "/state/locks/" + signer + ".lock"},
            {"nonce_before", "0"},
            {"height_before", 10}};
}

inline std::string sent(const std::string& hash) {
    return json{{"success", true}, {"tx_hash", hash}, {"secrets", json::array()}, {"error", ""}}.dump();
}

// Mocked C replies must outlive the call: the mock store keeps a pointer to the string.
inline const std::string& keep(std::string text) {
    static std::deque<std::string> store;
    store.push_back(std::move(text));
    return store.back();
}

// The context the harness configures; every FFI call must carry it (plus the module's budget_ms).
inline json configuredCtx() {
    return {{"deployment", "/d/deployment.json"}, {"state_dir", "/state"}, {"wallet_home", "/wallet"}};
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
        module.configure(configuredCtx());
    }

    ~Harness() { auction_engine::setClockForTests(nullptr); }

    void ffi(const std::string& name, const std::string& reply) {
        context.mockCFunction(name).returns(keep(reply));
    }

    /// A plan of the given steps; tx_begin, tx_end and the send succeed.
    void preparedSteps(const json& steps) {
        ffi("auction_ffi_prepare", ok({{"steps", steps}, {"facts", json::array()}}));
        ffi("auction_ffi_tx_begin", ok(tracking(steps.at(0).at("signers").at(0).get<std::string>())));
        ffi("auction_ffi_tx_end", ok(json::object()));
        context.mockModule("lez_core", SEND).returns(sent("ab"));
    }

    void prepared(int steps) {
        json list = json::array();
        for (int i = 0; i < steps; ++i) list.push_back(step(i == 0 ? SIGNER_A : SIGNER_B));
        preparedSteps(list);
    }

    int calls(const std::string& ffiName) { return context.cFunctionCallCount(ffiName); }

    std::vector<json> requests(const std::string& ffiName) {
        std::vector<json> out;
        for (const std::string& text : mock_ffi::state().requests[ffiName]) out.push_back(json::parse(text));
        return out;
    }

    json lastRequest(const std::string& ffiName) {
        return json::parse(mock_ffi::state().requests[ffiName].back());
    }

    /// logos-test records C calls but not their arguments; the mock keeps every request, so this checks
    /// that one call of `ffiName` got exactly `expected`.
    bool cFunctionCalledWith(const std::string& ffiName, const json& expected) {
        for (const json& request : requests(ffiName)) {
            if (request == expected) return true;
        }
        return false;
    }

    LogosMap submit() { return module.submit({{"action", "token_transfer"}}); }
};

}  // namespace harness
