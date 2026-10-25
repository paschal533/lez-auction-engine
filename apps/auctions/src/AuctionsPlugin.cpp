#include "AuctionsPlugin.h"
#include "AuctionsBackend.h"

AuctionsPlugin::AuctionsPlugin(QObject* parent)
    : QObject(parent)
{
}

AuctionsPlugin::~AuctionsPlugin() = default;

void AuctionsPlugin::initLogos(LogosAPI* api)
{
    if (m_backend) return;
    m_backend = new AuctionsBackend(api, this);
    setBackend(m_backend);
}
