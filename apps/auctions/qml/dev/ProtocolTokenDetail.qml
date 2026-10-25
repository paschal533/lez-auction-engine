import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format

// Dev tools > Protocol token (board v5 "Protocol token · admin, freeze trigger"): the demo's freeze trigger,
// `admin set-engine-params --params cli/params/demo-engine.json --disable-protocol-token` signed by
// engine-admin (scripts/lib/demo-scenario.sh). Without a protocol token no debt auction can ever start, so
// the next Check solvency freezes the host when unbacked debt is above the freeze threshold. The only
// destructive action in the app: an error notice (`devTokenWarning`), a red-outlined secondary button
// (`devDisableProtocolToken`), and a confirmation dialog that repeats the warning. Figures are the overview's.
Item {
    id: root
    objectName: "protocolTokenDetail"

    // The DevPage: act(spec), busy, active, signer.
    property var page: null
    property var overview: null

    readonly property var acc: overview && overview.accounting ? overview.accounting : null
    readonly property bool aboveThreshold: !!acc
        && Format.cmp(String(acc.unbacked_debt), String(acc.freeze_threshold)) > 0
    readonly property bool canAct: !!page && page.active && !page.busy && !!acc

    function warning(a) {
        if (!a)
            return ""
        const unbacked = Format.amount(String(a.unbacked_debt), 0)
        const threshold = Format.amount(String(a.freeze_threshold), 0)
        return Format.cmp(String(a.unbacked_debt), String(a.freeze_threshold)) > 0
            ? qsTr("The engine can no longer mint protocol tokens, so no debt auction can cover the %1 stable of unbacked debt. The next Check solvency freezes the host (unbacked debt is above the %2 freeze threshold).").arg(unbacked).arg(threshold)
            : qsTr("The engine can no longer mint protocol tokens, so no debt auction can start. Unbacked debt (%1 stable) is at or below the %2 freeze threshold, so Check solvency does not freeze the host yet.").arg(unbacked).arg(threshold)
    }

    function disable() {
        if (!canAct)
            return
        page.act({
            title: qsTr("Disable protocol token"),
            read: { view: "overview" },
            dialog: function(o) {
                return {
                    action: "disable_protocol_token",
                    title: qsTr("Disable protocol token"),
                    subtitle: qsTr("Freeze trigger · cannot be undone from the app"),
                    facts: [
                        { label: qsTr("Signed by"), value: "engine-admin" },
                        { label: qsTr("Unbacked debt"), value: String(o.accounting.unbacked_debt), decimals: 0, unit: qsTr("stable") },
                        { label: qsTr("Freeze threshold"), value: String(o.accounting.freeze_threshold), decimals: 0, unit: qsTr("stable") }
                    ],
                    note: root.warning(o.accounting),
                    reveals: [qsTr("The engine admin engine-admin"), qsTr("The new engine parameters"), qsTr("When it was sent")],
                    confirmLabel: qsTr("Disable protocol token")
                }
            },
            submit: function() { return { action: "disable_protocol_token", admin: "engine-admin" } }
        })
    }

    Flickable {
        id: flick
        anchors.fill: parent
        clip: true
        contentWidth: width
        contentHeight: Math.max(height, content.implicitHeight)
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: LogosScrollBar { policy: flick.contentHeight > flick.height ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff }

        ColumnLayout {
            id: content
            width: flick.width
            spacing: Theme.spacing.medium

            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.spacing.medium
                LogosText {
                    objectName: "devTokenTitle"
                    text: qsTr("Protocol token")
                    font.pixelSize: Theme.typography.panelTitleText
                    font.weight: Theme.typography.weightMedium
                    color: Theme.palette.text
                }
                LogosBadge {
                    text: qsTr("Admin · engine-admin")
                    color: Theme.palette.error
                }
                Item { Layout.fillWidth: true }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: Theme.palette.borderSubtle
            }

            InsetCard {
                objectName: "devTokenCard"
                Layout.fillWidth: true
                spacing: Theme.spacing.medium

                LogosNotice {
                    objectName: "devTokenWarning"
                    Layout.fillWidth: true
                    severity: LogosNotice.Error
                    title: qsTr("Disable the protocol token")
                    message: root.acc ? root.warning(root.acc) : qsTr("Reading the engine's accounting…")
                    shown: true
                }

                GridLayout {
                    Layout.fillWidth: true
                    columns: 3
                    columnSpacing: Theme.spacing.large
                    uniformCellWidths: true
                    Fact {
                        objectName: "devTokenUnbacked"
                        Layout.fillWidth: true
                        label: qsTr("Unbacked debt")
                        value: root.acc ? String(root.acc.unbacked_debt) : ""
                        decimals: 0
                        unit: qsTr("stable")
                    }
                    Fact {
                        objectName: "devTokenThreshold"
                        Layout.fillWidth: true
                        label: qsTr("Freeze threshold")
                        value: root.acc ? String(root.acc.freeze_threshold) : ""
                        decimals: 0
                        unit: qsTr("stable")
                    }
                    Fact {
                        objectName: "devTokenSigner"
                        Layout.fillWidth: true
                        label: qsTr("Signed by")
                        value: "engine-admin"
                    }
                }

                LogosButton {
                    id: disableButton
                    objectName: "devDisableProtocolToken"
                    implicitHeight: 40
                    radius: Theme.spacing.radiusLarge
                    text: qsTr("Disable protocol token")
                    variant: LogosButton.Variant.Secondary
                    enabled: root.canAct
                    onClicked: root.disable()
                    // The destructive action: red outline and label on the secondary button.
                    Component.onCompleted: {
                        labelItem.color = Qt.binding(function() {
                            return enabled ? Theme.palette.error : Theme.palette.textMuted
                        })
                        backgroundItem.border.color = Qt.binding(function() {
                            return enabled ? Theme.palette.error : Theme.palette.border
                        })
                    }
                }
            }

            Item { Layout.fillHeight: true }
        }
    }
}
