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
constexpr const char* KIND_CHAIN = "chain";
constexpr const char* DEFAULT_WALLET_SUBDIR = "/.lee/wallet";
// One rescan page: one getBlockRange batch (ScanSettings::default().batch in auction_sdk) plus the head and
// anchor reads, so a page needs few reads of the ~14 s a view call gets (budget.h).
// auction_ffi caps a page at 500 blocks; a caller may ask for any size up to that.
constexpr int DEFAULT_RESCAN_PAGE_BLOCKS = 100;
// auction_sdk ConfirmSettings::default().drop_after_blocks; the ctx may override it (auction_ffi ctx.rs).
constexpr int DEFAULT_DROP_AFTER_BLOCKS = 5;
// What became of a step whose begin or send failed (R27, R53). `not_sent`: no lock was taken and nothing reached
// the node; `dropped`: the lock was released before the send was started, nothing reached the node; `unknown`:
// the send was started and its fate is not known (lez_core failed, did not answer or answered without a hash:
// the request may have reached the sequencer), the lock is kept for the staleness rule.
constexpr const char* OUTCOME_NOT_SENT = "not_sent";
constexpr const char* OUTCOME_DROPPED = "dropped";
constexpr const char* OUTCOME_UNKNOWN = "unknown";
// The fields of an account picker row (auction_sdk views::AccountView).
constexpr const char* ACCOUNT_FIELDS[] = {"label", "id", "role", "stable_balance", "coll_balance", "holding"};

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

/// What a successful reply's `value` must be: an object for every call but the `accounts` view (a list).
enum class ValueShape { Object, Array };

/// One auction_ffi call: dump, call, copy, free, parse the `{ok, value | error}` envelope.
FfiResult callFfi(FfiOp op, const json& request, ValueShape shape = ValueShape::Object) {
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
    if (shape == ValueShape::Array) {
        if (value == envelope.end() || !value->is_array()) {
            return failure(KIND_BAD_REPLY, "auction_ffi reply has no list \"value\"");
        }
    } else if (value == envelope.end() || !value->is_object()) {
        return failure(KIND_BAD_REPLY, "auction_ffi reply has no object \"value\"");
    }
    FfiResult r;
    r.ok = true;
    r.value = *value;
    return r;
}

/// A call under the module budget: refused with less than MIN_FFI_START_MS left, else the context carries
/// what is left, less `keepMs` (what the module keeps for the reply and later steps), as `budget_ms`.
FfiResult budgetedCall(FfiOp op, json request, const json& ctx, const Budget& budget,
                       std::int64_t keepMs, ValueShape shape = ValueShape::Object) {
    if (!budget.hasAtLeast(auction_engine::MIN_FFI_START_MS)) {
        return failure(KIND_BUDGET, "the call's time budget ran out before the next step could start");
    }
    request["ctx"] = ctx;
    request["ctx"]["budget_ms"] =
        std::max(auction_engine::MIN_FFI_BUDGET_MS, budget.remainingMs() - keepMs);
    return callFfi(op, request, shape);
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

/// One view read under a fresh module budget; the configured context replaces any the caller sent.
/// Returns the FFI `value` or `{error, kind}`.
json viewCall(const json& ctx, const json& req, ValueShape shape = ValueShape::Object) {
    if (ctx.is_null()) return errorMap(KIND_NOT_CONFIGURED, "configure the module first");
    const Budget budget;
    const FfiResult r = budgetedCall(auction_ffi_view, req, ctx, budget, auction_engine::FFI_REPLY_MS, shape);
    return r.ok ? r.value : errorOf(r);
}

/// Where a sequence ended: `failedStep` was dropped or could not be sent, the steps before it are on chain and
/// the steps after it are discarded, never begun. Every sequence end reports the same three fields.
void markSequenceEnd(json& status, int failedStep, std::size_t stepCount, const std::string& outcome) {
    status["outcome"] = outcome;
    status["step"] = failedStep;
    status["includedSteps"] = failedStep;
    status["discardedSteps"] = static_cast<int>(stepCount) - failedStep - 1;
}

LogosMap withOutcome(LogosMap error, const char* outcome) {
    error["outcome"] = outcome;
    return error;
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
                                            auction_engine::FFI_REPLY_MS);
        if (!view.ok) return errorOf(view);
        m_ctx = stored;
        return {{"network", view.value.value("network", json())},
                {"dev", view.value.value("dev", false)},
                {"config", view.value.value("config", json())}};
    });
}

int AuctionEngineImpl::dropAfterBlocks(const json& ctx) {
    const auto it = ctx.find("drop_after_blocks");
    if (it == ctx.end() || !it->is_number_integer() || it->get<int>() < 1) return DEFAULT_DROP_AFTER_BLOCKS;
    return it->get<int>();
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
        // The app calls this as its heartbeat once the wallet is open, so no height reads as node trouble.
        if (height <= 0) return errorMap(KIND_CHAIN, "the node did not answer (or the wallet is closed)");
        return {{"height", height}};
    });
}

/// Checks the step, takes its primary-signer lock (tx_begin) under `ctx`, then sends it through lez_core. On
/// return without an error `tracking` and `hash` are set and the lock is held. Every error carries `outcome`:
/// `not_sent` when no lock was taken (bad step, tx_begin failed), else what the lock was ended with: `dropped`
/// (released: the send was never started) or `unknown` (kept: the send was started and its fate is not known,
/// Ruling R53).
LogosMap AuctionEngineImpl::beginAndSend(const json& ctx,
                                         const Budget& budget,
                                         const json& step,
                                         json& tracking,
                                         std::string& hash) {
    if (!hasStrings(step, "signers"))
        return withOutcome(errorMap(KIND_BAD_PLAN, "plan step has no signer"), OUTCOME_NOT_SENT);
    LezArgs args;
    try {
        args = toLezArgs(step);  // before the lock: a bad plan costs no lock
    } catch (const std::invalid_argument& e) {
        return withOutcome(errorMap(KIND_BAD_PLAN, e.what()), OUTCOME_NOT_SENT);
    }
    const FfiResult begun = budgetedCall(auction_ffi_tx_begin, {{"signer", step.at("signers").at(0)}}, ctx, budget,
                                         auction_engine::FFI_REPLY_MS + auction_engine::MIN_SEND_START_MS);
    if (!begun.ok) return withOutcome(errorOf(begun), OUTCOME_NOT_SENT);
    tracking = begun.value;

    if (!budget.hasAtLeast(auction_engine::MIN_SEND_START_MS)) {
        return withOutcome(errorMap(KIND_BUDGET, "not enough of the call's time budget is left to send the transaction"
                                         + endTracking(ctx, tracking, OUTCOME_DROPPED)),
                           OUTCOME_DROPPED);
    }

    logos::CallError callError;
    const std::string raw = modules().lez_core.send_generic_public_transaction(
        args.accountIds, args.signers, args.instruction, args.programIdHex, &callError);
    if (!callError.ok()) {
        return withOutcome(errorMap(KIND_SEND, "lez_core did not answer the send: it may or may not have reached the node"
                                         + endTracking(ctx, tracking, OUTCOME_UNKNOWN)),
                           OUTCOME_UNKNOWN);
    }
    const json sent = json::parse(raw, nullptr, false);
    if (!sent.is_object() || !sent.contains("success") || !sent.at("success").is_boolean()) {
        return withOutcome(errorMap(KIND_SEND, "lez_core sent an unreadable reply: the send may or may not have reached the node"
                                         + endTracking(ctx, tracking, OUTCOME_UNKNOWN)),
                           OUTCOME_UNKNOWN);
    }
    if (!sent.at("success").get<bool>()) {
        // lez_core's failure may come after the request left (a proxy 502, a response timeout): a retry could act
        // twice, so the lock is kept until the staleness rule (nonce moved, or drop_after_blocks) frees it.
        const std::string why = textOf(sent, "error");
        return withOutcome(errorMap(KIND_SEND, (why.empty() ? std::string("lez_core reported the send failed") : why)
                                         + ": the send may or may not have reached the node"
                                         + endTracking(ctx, tracking, OUTCOME_UNKNOWN)),
                           OUTCOME_UNKNOWN);
    }
    hash = textOf(sent, "tx_hash");
    if (hash.empty()) {
        return withOutcome(errorMap(KIND_SEND, "lez_core reported success without a transaction hash: the send may have reached the node"
                                         + endTracking(ctx, tracking, OUTCOME_UNKNOWN)),
                           OUTCOME_UNKNOWN);
    }
    return json::object();
}

LogosMap AuctionEngineImpl::submit(const LogosMap& req) {
    return guarded([&]() -> LogosMap {
        if (m_ctx.is_null()) return errorMap(KIND_NOT_CONFIGURED, "configure the module first");
        const Budget budget;
        const FfiResult plan = budgetedCall(auction_ffi_prepare, req, m_ctx, budget,
                                            auction_engine::FFI_REPLY_MS + auction_engine::BEGIN_AND_SEND_MS);
        if (!plan.ok) return errorOf(plan);
        const json steps = plan.value.value("steps", json::array());
        if (!steps.is_array() || steps.empty()) return errorMap(KIND_BAD_PLAN, "the plan has no steps");

        json tracking;
        std::string hash;
        const LogosMap sent = beginAndSend(m_ctx, budget, steps.at(0), tracking, hash);
        if (sent.contains("error")) {
            LogosMap failed = sent;
            markSequenceEnd(failed, 0, steps.size(), textOf(sent, "outcome"));
            return failed;
        }

        const std::string txId = "tx" + std::to_string(++m_lastTxNumber);
        // The entry keeps the ctx it was sent under: a configure while it is in flight must not poll another chain.
        m_pending[txId] = {{"steps", steps}, {"step", 0}, {"tracking", tracking}, {"hash", hash}, {"ctx", m_ctx}};
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
        const json ctx = entry.at("ctx");
        const Budget budget;

        const FfiResult polled = budgetedCall(auction_ffi_tx_poll, {{"tracking", entry.at("tracking")}}, ctx, budget,
                                              auction_engine::FFI_REPLY_MS + auction_engine::BEGIN_AND_SEND_MS);
        if (!polled.ok) return errorOf(polled);

        const int step = entry.at("step").get<int>();
        const std::size_t stepCount = entry.at("steps").size();
        LogosMap status = {{"txId", txId}, {"hash", entry.at("hash")}, {"step", step}, {"steps", stepCount}};
        const std::string state = textOf(polled.value, "state");

        if (state == "pending") {
            status["state"] = "pending";
            status["blocks_waited"] = polled.value.value("blocks_waited", 0);
            status["drop_after_blocks"] = dropAfterBlocks(ctx);
            return status;
        }
        if (state == "dropped") {
            // The sequence stops at the dropped step: the steps after it are discarded, never begun.
            status["state"] = "dropped";
            status["height"] = polled.value.value("height", 0);
            status["drop_after_blocks"] = dropAfterBlocks(ctx);
            markSequenceEnd(status, step, stepCount, OUTCOME_DROPPED);
            const std::string note = endTracking(ctx, entry.at("tracking"), "dropped");
            m_pending.erase(found);
            if (!note.empty()) status["note"] = note;
            return status;
        }
        if (state != "included") {
            return errorMap(KIND_BAD_REPLY, "tx_poll returned the state \"" + state + "\"");
        }

        const std::string nonceAfter = textOf(polled.value, "nonce_after");
        const std::string note = endTracking(ctx, entry.at("tracking"), "included");
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
            beginAndSend(ctx, budget, entry.at("steps").at(static_cast<std::size_t>(step) + 1), tracking, hash);
        if (sent.contains("error")) {
            // Step `step` is on chain, the next one is not sent and nothing is waited on: the sequence ends here.
            LogosMap failed = sent;
            markSequenceEnd(failed, step + 1, stepCount, textOf(sent, "outcome"));
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

LogosMap AuctionEngineImpl::view(const LogosMap& req) {
    return guarded([&]() -> LogosMap {
        const std::string name = textOf(req, "view");
        if (name.empty()) return errorMap(KIND_BAD_REQUEST, "the request names no \"view\"");
        // A map reply cannot carry the accounts view's list.
        if (name == "accounts") return errorMap(KIND_BAD_REQUEST, "the accounts view is a list: call accounts()");
        return viewCall(m_ctx, req);
    });
}

LogosList AuctionEngineImpl::accounts() {
    const json result = guarded([&]() -> LogosMap {
        const json rows = viewCall(m_ctx, {{"view", "accounts"}}, ValueShape::Array);
        if (!rows.is_array()) return rows;  // {error, kind}
        json picker = json::array();
        for (const json& row : rows) {
            if (!row.is_object()) return errorMap(KIND_BAD_REPLY, "an accounts row is not an object");
            json entry = json::object();
            for (const char* field : ACCOUNT_FIELDS) entry[field] = row.value(field, json());
            picker.push_back(entry);
        }
        return {{"rows", picker}};
    });
    // A list has no room for {error, kind}: a failure is the list's only row.
    return result.contains("error") ? json::array({result}) : result.at("rows");
}

LogosMap AuctionEngineImpl::rescan(const LogosMap& req) {
    return guarded([&]() -> LogosMap {
        json page = {{"view", "scan"}, {"max_blocks", DEFAULT_RESCAN_PAGE_BLOCKS}};
        if (req.is_object() && req.contains("max_blocks")) page["max_blocks"] = req.at("max_blocks");
        return viewCall(m_ctx, page);
    });
}
