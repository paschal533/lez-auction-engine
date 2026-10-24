#include "auction_engine_impl.h"

#include <algorithm>
#include <cstdlib>
#include <stdexcept>
#include <string>

#include "auction_ffi.h"
#include "budget.h"
#include "lez_args.h"
#include "logos_sdk.h"

namespace {

using json = nlohmann::json;
using auction_engine::Budget;

constexpr const char* KIND_BUDGET = "budget";
constexpr const char* KIND_NOT_CONFIGURED = "not_configured";
constexpr const char* KIND_BAD_REQUEST = "bad_request";
constexpr const char* KIND_BAD_PLAN = "bad_plan";
constexpr const char* KIND_BAD_REPLY = "bad_reply";
constexpr const char* KIND_SEND = "send";
constexpr const char* KIND_FFI = "ffi";
constexpr const char* KIND_UNKNOWN_TX = "unknown_tx";
constexpr const char* KIND_WALLET = "wallet";
constexpr const char* DEFAULT_WALLET_SUBDIR = "/.lee/wallet";

using FfiOp = char* (*)(const char*);

struct FfiResult {
    bool ok = false;
    json value;
    std::string kind;
    std::string message;
};

LogosMap errorMap(const std::string& kind, const std::string& message) {
    return {{"error", message}, {"kind", kind}};
}

LogosMap errorOf(const FfiResult& r) {
    return errorMap(r.kind, r.message);
}

FfiResult failure(const std::string& kind, const std::string& message) {
    FfiResult r;
    r.kind = kind;
    r.message = message;
    return r;
}

std::string textOf(const json& object, const char* key) {
    if (!object.is_object()) return {};
    const auto it = object.find(key);
    return it != object.end() && it->is_string() ? it->get<std::string>() : std::string();
}

/// One auction_ffi call: dump, call, copy, free, parse the `{ok, value | error}` envelope.
FfiResult callFfi(FfiOp op, const json& request) {
    const std::string payload = request.dump();
    char* raw = op(payload.c_str());
    if (raw == nullptr) return failure(KIND_BAD_REPLY, "auction_ffi returned no reply");
    const std::string text(raw);
    auction_ffi_free(raw);

    const json envelope = json::parse(text, nullptr, false);
    if (!envelope.is_object() || !envelope.contains("ok") || !envelope.at("ok").is_boolean()) {
        return failure(KIND_BAD_REPLY, "auction_ffi reply is not an envelope");
    }
    if (!envelope.at("ok").get<bool>()) {
        const json error = envelope.value("error", json::object());
        const std::string kind = textOf(error, "kind");
        const std::string message = textOf(error, "message");
        return failure(kind.empty() ? KIND_FFI : kind,
                       message.empty() ? "auction_ffi reported an error without a message" : message);
    }
    const auto value = envelope.find("value");
    if (value == envelope.end() || !value->is_object()) {
        return failure(KIND_BAD_REPLY, "auction_ffi reply has no object \"value\"");
    }
    FfiResult r;
    r.ok = true;
    r.value = *value;
    return r;
}

/// A call under the module budget: refused with less than MIN_FFI_START_MS left, else the context carries
/// what is left, less the call kind's read overrun, as `budget_ms`.
FfiResult budgetedCall(FfiOp op, json request, const json& ctx, const Budget& budget,
                       std::int64_t readOverrunMs) {
    if (!budget.hasAtLeast(auction_engine::MIN_FFI_START_MS)) {
        return failure(KIND_BUDGET, "the call's time budget ran out before the next step could start");
    }
    request["ctx"] = ctx;
    request["ctx"]["budget_ms"] =
        std::max(auction_engine::MIN_FFI_BUDGET_MS, budget.remainingMs() - readOverrunMs);
    return callFfi(op, request);
}

/// `tx_end` reads no node, so it needs no budget: the lock is released even when the budget is spent.
/// Returns "" or a note on why the release failed.
std::string endTracking(const json& ctx, const json& tracking, const char* outcome) {
    const FfiResult r = callFfi(auction_ffi_tx_end,
                                {{"ctx", ctx}, {"tracking", tracking}, {"outcome", outcome}});
    return r.ok ? std::string() : " (releasing the in-flight lock failed: " + r.message + ")";
}

/// No exception crosses a module method: a reply of the wrong type becomes a `bad_reply` error.
template <typename Body>
LogosMap guarded(Body body) {
    try {
        return body();
    } catch (const json::exception& e) {
        return errorMap(KIND_BAD_REPLY, std::string("unexpected reply type: ") + e.what());
    } catch (const std::exception& e) {
        return errorMap(KIND_BAD_REPLY, e.what());
    }
}

bool hasStrings(const json& object, const char* key) {
    const auto it = object.find(key);
    if (it == object.end() || !it->is_array() || it->empty()) return false;
    for (const json& entry : *it) {
        if (!entry.is_string()) return false;
    }
    return true;
}

}  // namespace

LogosMap AuctionEngineImpl::configure(const LogosMap& ctx) {
    return guarded([&]() -> LogosMap {
        if (!ctx.is_object() || textOf(ctx, "deployment").empty() || textOf(ctx, "state_dir").empty()) {
            return errorMap(KIND_BAD_REQUEST, "ctx needs \"deployment\" and \"state_dir\"");
        }
        // The module owns budget_ms: a caller-supplied one never reaches auction_ffi.
        json stored = ctx;
        stored.erase("budget_ms");
        const Budget budget;
        const FfiResult view = budgetedCall(auction_ffi_view, {{"view", "deployment"}}, stored, budget,
                                            auction_engine::VIEW_READ_OVERRUN_MS);
        if (!view.ok) return errorOf(view);
        m_ctx = stored;
        return {{"network", view.value.value("network", json())},
                {"dev", view.value.value("dev", false)},
                {"config", view.value.value("config", json())}};
    });
}

std::string AuctionEngineImpl::walletHome() const {
    const std::string configured = textOf(m_ctx, "wallet_home");
    if (!configured.empty()) return configured;
    if (const char* env = std::getenv("LEE_WALLET_HOME_DIR"); env != nullptr && *env != '\0') return env;
    const char* home = std::getenv("HOME");
    return std::string(home != nullptr ? home : "") + DEFAULT_WALLET_SUBDIR;
}

bool AuctionEngineImpl::walletIsOpen() {
    // lez_core refuses a second open; list_accounts and the height read are empty or 0 while it is closed.
    if (modules().lez_core.get_current_block_height() > 0) return true;
    const json accounts = modules().lez_core.list_accounts();
    return accounts.is_array() && !accounts.empty();
}

LogosMap AuctionEngineImpl::openWallet() {
    return guarded([&]() -> LogosMap {
        if (m_ctx.is_null()) return errorMap(KIND_NOT_CONFIGURED, "configure the module first");
        const std::string home = walletHome();
        const std::int64_t code = modules().lez_core.open(
            home + "/wallet_config.json", home + "/storage.json", home + "/statistics.json");
        if (code == 0) return {{"opened", true}};
        if (walletIsOpen()) return {{"opened", false}, {"alreadyOpen", true}};
        return errorMap(KIND_WALLET, "lez_core could not open the wallet under " + home + " (code "
                                         + std::to_string(code) + ")");
    });
}

LogosMap AuctionEngineImpl::blockHeight() {
    return guarded([&]() -> LogosMap {
        const std::int64_t height = modules().lez_core.get_current_block_height();
        if (height <= 0) return errorMap(KIND_WALLET, "the wallet is closed or the node is unreachable");
        return {{"height", height}};
    });
}

/// Checks the step, takes its primary-signer lock (tx_begin), then sends it through lez_core. On return without
/// an error `tracking` and `hash` are set and the lock is held. A failure releases the lock it took: `dropped`
/// when nothing reached the node, `unknown` (lock kept) when the send's outcome is not known.
LogosMap AuctionEngineImpl::beginAndSend(const Budget& budget,
                                         const json& step,
                                         json& tracking,
                                         std::string& hash) {
    if (!hasStrings(step, "signers")) return errorMap(KIND_BAD_PLAN, "plan step has no signer");
    LezArgs args;
    try {
        args = toLezArgs(step);  // before the lock: a bad plan costs no lock
    } catch (const std::invalid_argument& e) {
        return errorMap(KIND_BAD_PLAN, e.what());
    }
    const FfiResult begun = budgetedCall(auction_ffi_tx_begin, {{"signer", step.at("signers").at(0)}}, m_ctx,
                                         budget, auction_engine::TX_READ_OVERRUN_MS);
    if (!begun.ok) return errorOf(begun);
    tracking = begun.value;

    if (!budget.hasAtLeast(auction_engine::MIN_SEND_START_MS)) {
        return errorMap(KIND_BUDGET, "not enough of the call's time budget is left to send the transaction"
                                         + endTracking(m_ctx, tracking, "dropped"));
    }

    logos::CallError callError;
    const std::string raw = modules().lez_core.send_generic_public_transaction(
        args.accountIds, args.signers, args.instruction, args.programIdHex, &callError);
    if (!callError.ok()) {
        return errorMap(KIND_SEND, "lez_core did not answer the send: it may or may not have reached the node"
                                       + endTracking(m_ctx, tracking, "unknown"));
    }
    const json sent = json::parse(raw, nullptr, false);
    if (!sent.is_object() || !sent.contains("success") || !sent.at("success").is_boolean()) {
        return errorMap(KIND_SEND, "lez_core sent an unreadable reply: the send may or may not have reached the node"
                                       + endTracking(m_ctx, tracking, "unknown"));
    }
    if (!sent.at("success").get<bool>()) {
        const std::string why = textOf(sent, "error");
        return errorMap(KIND_SEND, (why.empty() ? std::string("lez_core refused the transaction") : why)
                                       + endTracking(m_ctx, tracking, "dropped"));
    }
    hash = textOf(sent, "tx_hash");
    if (hash.empty()) {
        return errorMap(KIND_SEND, "lez_core reported success without a transaction hash: the send may have reached the node"
                                       + endTracking(m_ctx, tracking, "unknown"));
    }
    return json::object();
}

LogosMap AuctionEngineImpl::submit(const LogosMap& req) {
    return guarded([&]() -> LogosMap {
        if (m_ctx.is_null()) return errorMap(KIND_NOT_CONFIGURED, "configure the module first");
        const Budget budget;
        const FfiResult plan = budgetedCall(auction_ffi_prepare, req, m_ctx, budget,
                                            auction_engine::VIEW_READ_OVERRUN_MS);
        if (!plan.ok) return errorOf(plan);
        const json steps = plan.value.value("steps", json::array());
        if (!steps.is_array() || steps.empty()) return errorMap(KIND_BAD_PLAN, "the plan has no steps");

        json tracking;
        std::string hash;
        const LogosMap sent = beginAndSend(budget, steps.at(0), tracking, hash);
        if (sent.contains("error")) return sent;

        const std::string txId = "tx" + std::to_string(++m_lastTxNumber);
        m_pending[txId] = {{"steps", steps}, {"step", 0}, {"tracking", tracking}, {"hash", hash}};
        return {{"txId", txId},
                {"hash", hash},
                {"what", textOf(steps.at(0), "what")},
                {"step", 0},
                {"steps", steps.size()},
                {"tracking", tracking}};
    });
}

LogosMap AuctionEngineImpl::txStatus(const LogosMap& req) {
    return guarded([&]() -> LogosMap {
        const std::string txId = textOf(req, "txId");
        const auto found = m_pending.find(txId);
        if (found == m_pending.end()) {
            return errorMap(KIND_UNKNOWN_TX, "no transaction in flight with id \"" + txId + "\"");
        }
        json& entry = found->second;
        const Budget budget;

        const FfiResult polled = budgetedCall(auction_ffi_tx_poll, {{"tracking", entry.at("tracking")}}, m_ctx,
                                              budget, auction_engine::TX_READ_OVERRUN_MS);
        if (!polled.ok) return errorOf(polled);

        const int step = entry.at("step").get<int>();
        const std::size_t stepCount = entry.at("steps").size();
        LogosMap status = {{"txId", txId}, {"hash", entry.at("hash")}, {"step", step}, {"steps", stepCount}};
        const std::string state = textOf(polled.value, "state");

        if (state == "pending") {
            status["state"] = "pending";
            status["blocks_waited"] = polled.value.value("blocks_waited", 0);
            return status;
        }
        if (state == "dropped") {
            status["state"] = "dropped";
            status["height"] = polled.value.value("height", 0);
            const std::string note = endTracking(m_ctx, entry.at("tracking"), "dropped");
            m_pending.erase(found);
            if (!note.empty()) status["note"] = note;
            return status;
        }
        if (state != "included") {
            return errorMap(KIND_BAD_REPLY, "tx_poll returned the state \"" + state + "\"");
        }

        const std::string nonceAfter = textOf(polled.value, "nonce_after");
        const std::string note = endTracking(m_ctx, entry.at("tracking"), "included");
        if (static_cast<std::size_t>(step) + 1 >= stepCount) {
            m_pending.erase(found);
            status["state"] = "included";
            status["nonce_after"] = nonceAfter;
            if (!note.empty()) status["note"] = note;
            return status;
        }

        json tracking;
        std::string hash;
        const LogosMap sent =
            beginAndSend(budget, entry.at("steps").at(static_cast<std::size_t>(step) + 1), tracking, hash);
        if (sent.contains("error")) {
            // Step `step` is on chain, the next one is not sent and nothing is waited on: the sequence ends here.
            LogosMap failed = sent;
            failed["step"] = step + 1;
            failed["includedSteps"] = step + 1;
            if (!note.empty()) failed["note"] = note;
            m_pending.erase(found);
            return failed;
        }
        entry["step"] = step + 1;
        entry["tracking"] = tracking;
        entry["hash"] = hash;
        status["state"] = "next";
        status["step"] = step + 1;
        status["hash"] = hash;
        status["tracking"] = tracking;
        if (!note.empty()) status["note"] = note;
        return status;
    });
}
