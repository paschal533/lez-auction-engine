#ifndef AUCTIONS_PLUGIN_H
#define AUCTIONS_PLUGIN_H

#include <QObject>
#include <QString>
#include <QtPlugin>
#include "AuctionsPluginInterface.h"
#include "LogosViewPluginBase.h"

class LogosAPI;
class AuctionsBackend;

// Plugin entry point: owns the backend and lets the generated view-plugin base expose it to ui-host.
class AuctionsPlugin : public QObject,
                       public AuctionsPluginInterface,
                       public AuctionsBackendViewPluginBase
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID AuctionsPluginInterface_iid FILE "../metadata.json")
    Q_INTERFACES(AuctionsPluginInterface)

public:
    explicit AuctionsPlugin(QObject* parent = nullptr);
    ~AuctionsPlugin() override;

    QString name() const override { return "auctions"; }
    QString version() const override { return "0.1.0"; }

    // Called by ui-host after the plugin loads.
    Q_INVOKABLE void initLogos(LogosAPI* api);

private:
    AuctionsBackend* m_backend = nullptr;
};

#endif // AUCTIONS_PLUGIN_H
