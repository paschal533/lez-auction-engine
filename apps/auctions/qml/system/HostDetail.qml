import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format

// Host (board v5): the host's ProtocolParameters as `host status` shows them (overview.host: frozen, admin,
// freeze authority, market oracle, minimum CR, oracle age limit). `frozen` is the current state (the engine's
// host_frozen_by_engine only means "has ever frozen"). Frozen: a LogosNotice Error "New debt is disabled"
// (`hostFrozenNotice`); auctions and claims keep working, opening debt on the host does not.
Item {
    id: root
    objectName: "hostDetail"

    property var overview: null

    readonly property var host: overview && overview.host ? overview.host : null
    readonly property var acc: overview && overview.accounting ? overview.accounting : null
    readonly property bool frozen: !!host && host.frozen === true

    function amount(v) { return Format.amount(String(v), 0) }

    ColumnLayout {
        anchors.fill: parent
        spacing: Theme.spacing.medium

        RowLayout {
            spacing: Theme.spacing.medium
            LogosText {
                objectName: "hostTitle"
                text: qsTr("Host")
                font.pixelSize: Theme.typography.panelTitleText
                font.weight: Theme.typography.weightMedium
                color: Theme.palette.text
            }
            LogosBadge {
                objectName: "hostBadge"
                visible: !!root.host
                text: root.frozen ? qsTr("Frozen") : qsTr("Live")
                color: root.frozen ? Theme.palette.error : Theme.palette.success
            }
        }

        LogosNotice {
            objectName: "hostFrozenNotice"
            Layout.fillWidth: true
            severity: LogosNotice.Error
            shown: root.frozen
            title: qsTr("New debt is disabled")
            message: !root.acc ? qsTr("Auctions and claims keep working; opening debt on the host does not.")
                     : (Format.cmp(String(root.acc.unbacked_debt), String(root.acc.freeze_threshold)) > 0
                        ? qsTr("Unbacked debt %1 stable is above the %2 threshold. ").arg(root.amount(root.acc.unbacked_debt)).arg(root.amount(root.acc.freeze_threshold))
                        : qsTr("Unbacked debt %1 stable, threshold %2. ").arg(root.amount(root.acc.unbacked_debt)).arg(root.amount(root.acc.freeze_threshold)))
                       + qsTr("Auctions and claims keep working; opening debt on the host does not. Only the host's freeze authority unfreezes.")
        }

        InsetCard {
            Layout.fillWidth: true
            GridLayout {
                Layout.fillWidth: true
                columns: 3
                columnSpacing: Theme.spacing.large
                rowSpacing: Theme.spacing.medium
                uniformCellWidths: true
                Fact {
                    objectName: "hostFrozen"
                    Layout.fillWidth: true
                    label: qsTr("Frozen")
                    value: !root.host ? "" : root.frozen ? qsTr("Yes") : qsTr("No")
                    valueColor: root.frozen ? Theme.palette.error : Theme.palette.text
                }
                Fact {
                    objectName: "hostMinCr"
                    Layout.fillWidth: true
                    label: qsTr("Minimum CR")
                    // Ray percent, floored like the CLI (R32).
                    value: root.host ? Format.rayPercent(String(root.host.min_cr_ray), 0) + "%" : ""
                }
                Fact {
                    objectName: "hostMaxAge"
                    Layout.fillWidth: true
                    label: qsTr("Oracle price may be")
                    value: root.host ? qsTr("%1 min old").arg(Math.floor(Number(root.host.max_oracle_age_ms) / 60000)) : ""
                }
                Fact {
                    objectName: "hostAdmin"
                    Layout.fillWidth: true
                    label: qsTr("Admin")
                    value: root.host ? Format.shortId(String(root.host.admin)) : ""
                }
                Fact {
                    objectName: "hostFreezeAuthority"
                    Layout.fillWidth: true
                    label: qsTr("Freeze authority")
                    value: root.host ? Format.shortId(String(root.host.freeze_authority)) : ""
                }
                Fact {
                    objectName: "hostOracle"
                    Layout.fillWidth: true
                    label: qsTr("Market oracle")
                    value: root.host ? Format.shortId(String(root.host.market_oracle)) : ""
                }
            }
        }

        LogosText {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            text: qsTr("Check solvency (Accounting) freezes new debt when unbacked debt stays above the freeze threshold. The engine never unfreezes; only the host's freeze authority does.")
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textSecondary
        }

        Item { Layout.fillHeight: true }
    }
}
