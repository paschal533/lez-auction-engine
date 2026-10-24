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

    /// The wallet's view of the chain height: `{height}`.
    LogosMap blockHeight();

    /// Plans the request with auction_ffi, takes the primary signer's lock and sends step 0 through lez_core.
    /// Returns `{txId, hash, what, step, steps, tracking}`; later steps are sent by txStatus.
    LogosMap submit(const LogosMap& req);

    /// `{txId}`: polls the transaction. `{state: "pending"}`, `{state: "next", step}` (inclusion sent the next
    /// step), or the final `{state: "included"}` / `{state: "dropped"}`.
    LogosMap txStatus(const LogosMap& req);

private:
    LogosMap beginAndSend(const auction_engine::Budget& budget,
                          const nlohmann::json& step,
                          nlohmann::json& tracking,
                          std::string& hash);
    std::string walletHome() const;
    bool walletIsOpen();

    nlohmann::json m_ctx;
    std::map<std::string, nlohmann::json> m_pending;
    std::uint64_t m_lastTxNumber = 0;
};
