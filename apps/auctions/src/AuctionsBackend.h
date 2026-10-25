#ifndef AUCTIONS_BACKEND_H
#define AUCTIONS_BACKEND_H

#include <memory>

#include <QString>
#include <QTimer>
#include <QVariantList>
#include <QVariantMap>

#include "rep_AuctionsBackend_source.h"

class LogosAPI;
struct LogosModules;

// Source side of the AuctionsBackend .rep contract. The auction logic lives in the auction_engine
// core module; this class owns the startup sequence (configure, open the wallet), the 5 s block poll
// (`overview` view -> blockHeight, nodeReachable), and forwards each slot to modules().auction_engine.
class AuctionsBackend : public AuctionsBackendSimpleSource {
    Q_OBJECT

public:
    explicit AuctionsBackend(LogosAPI* logosAPI = nullptr, QObject* parent = nullptr);
    ~AuctionsBackend() override;

    QVariantMap configure(QString deployment, QString stateDir, QString walletHome) override;
    QVariantMap submit(QVariantMap request) override;
    QVariantMap txStatus(QString txId) override;
    QVariantMap view(QVariantMap request) override;
    QVariantList accounts() override;

private:
    void start();
    void refreshChain();
    // Records the error of a failed engine reply in lastError; returns whether the reply failed.
    bool failed(const QVariantMap& reply);
    // A reply that worked proves the engine is healthy, so an earlier error no longer applies.
    void clearErrorOnSuccess(const QVariantMap& reply);
    // A `chain` failure (node or RPC not answering) sets nodeReachable false; returns whether it was one.
    bool noteChainFailure(const QVariantMap& reply);

    LogosAPI* m_logosAPI;
    std::unique_ptr<LogosModules> m_logos;
    QTimer m_chainTimer;
};

#endif // AUCTIONS_BACKEND_H
