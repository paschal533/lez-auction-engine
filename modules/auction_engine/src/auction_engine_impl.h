#pragma once

#include <cstdint>
#include <map>
#include <string>

#include <logos_json.h>
#include <logos_module_context.h>

namespace auction_engine {
class Budget;
}

// Universal Logos core module of the Auctions app. Rust auction_ffi plans transactions and tracks them (it
// never signs); this Qt-free adapter sends each planned step through lez_core and keeps the steps still to go.
// Every method returns a map; a failure is `{error, kind}`. The module never sleeps or loops waiting for a
// confirmation: the UI polls txStatus. Each method shares one 15 s time budget across its calls (budget.h).
class AuctionEngineImpl : public LogosModuleContext {
public:
    AuctionEngineImpl() = default;
    ~AuctionEngineImpl() = default;

    /// Stores the request context `{deployment, state_dir, wallet_home}` and returns the deployment view
    /// `{network, dev, config}` from auction_ffi.
    LogosMap configure(const LogosMap& ctx);

    /// Opens the wallet under `wallet_home`. A wallet that is already open is success:
    /// `{opened: false, alreadyOpen: true}`.
    LogosMap openWallet();

    /// The wallet's view of the chain height: `{height}`; no height is `{error, kind: "chain"}` (the app heartbeat).
    LogosMap blockHeight();

    /// Plans the request with auction_ffi, takes the primary signer's lock and sends step 0 through lez_core.
    /// Returns `{txId, hash, what, step, steps, tracking}`; later steps are sent by txStatus. If step 0 cannot
    /// begin or be sent: `{error, kind, step: 0, includedSteps: 0, discardedSteps}` (as every sequence end).
    LogosMap submit(const LogosMap& req);

    /// `{txId}`: polls the transaction. `{state: "pending", blocks_waited, drop_after_blocks}`, `{state: "next",
    /// step}` (inclusion sent the next step), or the final `{state: "included"}` / `{state: "dropped",
    /// drop_after_blocks, outcome: "dropped", includedSteps, discardedSteps}`. A step whose begin or send
    /// fails ends the sequence: `{error, kind, outcome: "not_sent"|"dropped"|"unknown", step, includedSteps,
    /// discardedSteps}`. Each
    /// step takes and releases its own signer's lock; a dropped step ends the sequence (later steps never begin).
    LogosMap txStatus(const LogosMap& req);

    /// Any auction_ffi view (`{view, ...}`, e.g. overview, live, auction, quote_bid) with the configured
    /// context; returns the view's JSON or `{error, kind}`. A ctx in the request is replaced.
    LogosMap view(const LogosMap& req);

    /// The account picker: one `{label, id, role, stable_balance, coll_balance, holding}` per row of the
    /// `accounts` view. A failure is the only row: `[{error, kind}]`.
    LogosList accounts();

    /// One page of the event scan (`{max_blocks}`, default 100, at most 500):
    /// `{scanned_to, head, done, restarted, note}`. The caller pages until `done`; never a full scan.
    LogosMap rescan(const LogosMap& req);

private:
    LogosMap beginAndSend(const auction_engine::Budget& budget,
                          const nlohmann::json& step,
                          nlohmann::json& tracking,
                          std::string& hash);
    std::string walletHome() const;
    /// The configured ctx's `drop_after_blocks`, else the SDK default 5; echoed in pending/dropped txStatus.
    int dropAfterBlocks() const;
    bool walletIsOpen();

    nlohmann::json m_ctx;
    std::map<std::string, nlohmann::json> m_pending;
    std::uint64_t m_lastTxNumber = 0;
};
