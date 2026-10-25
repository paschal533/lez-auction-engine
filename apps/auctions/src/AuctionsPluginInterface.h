#ifndef AUCTIONS_PLUGIN_INTERFACE_H
#define AUCTIONS_PLUGIN_INTERFACE_H

#include <QtPlugin>
#include "interface.h"

// Marker interface Qt's plugin loader uses to identify the Auctions plugin. The API surface
// (properties, slots) lives in AuctionsBackend.rep.
class AuctionsPluginInterface : public PluginInterface
{
public:
    virtual ~AuctionsPluginInterface() = default;
};

#define AuctionsPluginInterface_iid "org.logos.AuctionsPluginInterface"
Q_DECLARE_INTERFACE(AuctionsPluginInterface, AuctionsPluginInterface_iid)

#endif // AUCTIONS_PLUGIN_INTERFACE_H
