#include "AuctionsBackend.h"

#include <QTimer>

#include "logos_api.h"
#include "logos_sdk.h"

namespace {
const char ENV_DEPLOYMENT[] = "AUCTIONS_DEPLOYMENT";
const char ENV_STATE_DIR[] = "AUCTIONS_STATE_DIR";
const char ENV_WALLET_HOME[] = "LEE_WALLET_HOME_DIR";
// The header's block height and node notice: one cheap height read per interval (R28).
const int CHAIN_POLL_MS = 5000;
const char KIND_CHAIN[] = "chain";

QVariantMap errorReply(const QString& message, const QString& kind)
{
    return {{QStringLiteral("error"), message}, {QStringLiteral("kind"), kind}};
}
}

AuctionsBackend::AuctionsBackend(LogosAPI* logosAPI, QObject* parent)
    : AuctionsBackendSimpleSource(parent),
      m_logosAPI(logosAPI ? logosAPI : new LogosAPI("auctions", this)),
      m_logos(std::make_unique<LogosModules>(m_logosAPI))
{
    setNodeReachable(false);
    m_chainTimer.setInterval(CHAIN_POLL_MS);
    connect(&m_chainTimer, &QTimer::timeout, this, &AuctionsBackend::refreshChain);
    QTimer::singleShot(0, this, &AuctionsBackend::start);
}

AuctionsBackend::~AuctionsBackend() = default;

bool AuctionsBackend::failed(const QVariantMap& reply)
{
    if (!reply.contains(QStringLiteral("error")))
        return false;
    setLastError(reply.value(QStringLiteral("error")).toString());
    return true;
}

void AuctionsBackend::start()
{
    const QString deployment = qEnvironmentVariable(ENV_DEPLOYMENT);
    const QString stateDir = qEnvironmentVariable(ENV_STATE_DIR);
    if (deployment.isEmpty() || stateDir.isEmpty()) {
        setLastError(QStringLiteral("Set AUCTIONS_DEPLOYMENT and AUCTIONS_STATE_DIR to start the app."));
        return;
    }
    if (failed(configure(deployment, stateDir, qEnvironmentVariable(ENV_WALLET_HOME))))
        return;
    if (failed(m_logos->auction_engine.openWallet()))
        return;

    setWalletOpen(true);
    refreshChain();
    m_chainTimer.start();
}

void AuctionsBackend::refreshChain()
{
    // R28: the heartbeat is the module's cheap height read; screens fetch the views they show themselves.
    const QVariantMap reply = m_logos->auction_engine.blockHeight();
    if (reply.contains(QStringLiteral("error"))) {
        // `chain`: the node is not answering; the header says so and the next good beat recovers.
        // Another kind is an engine fault, shown once: the same error is not re-set every beat.
        if (!noteChainFailure(reply)) {
            const QString error = reply.value(QStringLiteral("error")).toString();
            if (error != m_lastBeatError)
                failed(reply);
            m_lastBeatError = error;
        }
        return;
    }
    m_lastBeatError.clear();
    setNodeReachable(true);
    setBlockHeight(reply.value(QStringLiteral("height")).toInt());
    clearErrorOnSuccess(reply);
}

bool AuctionsBackend::noteChainFailure(const QVariantMap& reply)
{
    if (reply.value(QStringLiteral("kind")).toString() != QLatin1String(KIND_CHAIN))
        return false;
    setNodeReachable(false);
    return true;
}

void AuctionsBackend::clearErrorOnSuccess(const QVariantMap& reply)
{
    if (!reply.contains(QStringLiteral("error")) && !lastError().isEmpty())
        setLastError(QString());
}

QVariantMap AuctionsBackend::configure(QString deployment, QString stateDir, QString walletHome)
{
    QVariantMap ctx{{QStringLiteral("deployment"), deployment},
                    {QStringLiteral("state_dir"), stateDir}};
    if (!walletHome.isEmpty())
        ctx.insert(QStringLiteral("wallet_home"), walletHome);

    const QVariantMap reply = m_logos->auction_engine.configure(ctx);
    if (failed(reply))
        return reply;
    setLastError(QString());
    setNetwork(reply.value(QStringLiteral("network")).toString());
    setDevNetwork(reply.value(QStringLiteral("dev")).toBool());
    return reply;
}

QVariantMap AuctionsBackend::submit(QVariantMap request)
{
    if (!walletOpen())
        return errorReply(QStringLiteral("The wallet is not open."), QStringLiteral("wallet"));
    // The caller shows this reply's error itself; lastError is not set again (no double banner).
    const QVariantMap reply = m_logos->auction_engine.submit(request);
    noteChainFailure(reply);
    clearErrorOnSuccess(reply);
    return reply;
}

QVariantMap AuctionsBackend::txStatus(QString txId)
{
    const QVariantMap reply =
        m_logos->auction_engine.txStatus(QVariantMap{{QStringLiteral("txId"), txId}});
    noteChainFailure(reply);
    clearErrorOnSuccess(reply);
    return reply;
}

QVariantMap AuctionsBackend::view(QVariantMap request)
{
    // The screen shows a failed view itself (StateNotice); only node trouble reaches the header.
    const QVariantMap reply = m_logos->auction_engine.view(request);
    noteChainFailure(reply);
    return reply;
}

QVariantMap AuctionsBackend::rescan(QVariantMap request)
{
    // One page; the Live screen pages until done and shows a failure itself.
    const QVariantMap reply = m_logos->auction_engine.rescan(request);
    noteChainFailure(reply);
    return reply;
}

QVariantList AuctionsBackend::accounts()
{
    // A failure arrives in-band as the only row, [{error, kind}]; the picker shows it.
    const QVariantList rows = m_logos->auction_engine.accounts();
    if (rows.size() == 1)
        noteChainFailure(rows.first().toMap());
    return rows;
}
