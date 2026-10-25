#include "AuctionsBackend.h"

#include <QTimer>

#include "logos_api.h"
#include "logos_sdk.h"

namespace {
const char ENV_DEPLOYMENT[] = "AUCTIONS_DEPLOYMENT";
const char ENV_STATE_DIR[] = "AUCTIONS_STATE_DIR";
const char ENV_WALLET_HOME[] = "LEE_WALLET_HOME_DIR";
// The node is read through the wallet, so one height read per interval is enough for the header.
const int CHAIN_POLL_MS = 15000;

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
    const QVariantMap reply = m_logos->auction_engine.blockHeight();
    const bool reachable = !reply.contains(QStringLiteral("error"));
    setNodeReachable(reachable);
    if (reachable)
        setBlockHeight(reply.value(QStringLiteral("height")).toInt());
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
    const QVariantMap reply = m_logos->auction_engine.submit(request);
    failed(reply);
    return reply;
}

QVariantMap AuctionsBackend::txStatus(QString txId)
{
    const QVariantMap reply =
        m_logos->auction_engine.txStatus(QVariantMap{{QStringLiteral("txId"), txId}});
    failed(reply);
    return reply;
}

QVariantMap AuctionsBackend::view(QVariantMap)
{
    return errorReply(QStringLiteral("not yet"), QStringLiteral("not_ready"));
}

QVariantList AuctionsBackend::accounts()
{
    return {};
}
