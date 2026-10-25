import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format

// Surplus auction (board v5): sells a lot of the buffer's stable for protocol tokens, which are burned. The
// facts are overview.accounting's (buffer, lot left, the nonce the next auction takes). `startSurplus` is
// refused while one is live, with the SDK's reason (ops/system.rs refuse_live); the engine checks the rest
// (no unbacked debt, buffer at its target plus a lot, the delay) and a refusal shows on the page. While one is
// live, the take card (`takeSurplusAmount`, `takeSurplusSubmit`, `takeSurplusBurnNote`).
Item {
    id: root
    objectName: "surplusDetail"

    property var page: null
    property var backend: null
    property var overview: null

    readonly property var acc: overview && overview.accounting ? overview.accounting : null
    readonly property bool live: !!acc && acc.surplus_live === true
    readonly property bool hasSigner: !!(page && page.signer)
    readonly property bool canStart: !!acc && !live && hasSigner && !page.busy

    function start() {
        const caller = page.activeAccount
        const from = page.signer ? String(page.signer.id) : ""
        page.act({
            title: qsTr("Start surplus auction"),
            read: { view: "overview" },
            // The fresh read still has to show no live surplus auction.
            check: function(o) { return !!o.accounting && !o.accounting.surplus_live },
            dialog: function(o) {
                return {
                    action: "start_surplus",
                    from: from,
                    facts: [
                        { label: qsTr("Buffer"), value: String(o.accounting.buffer), decimals: 0, unit: qsTr("stable") },
                        { label: qsTr("Auction nonce"), value: String(o.accounting.surplus_nonce) }
                    ],
                    note: qsTr("Settles what the buffer can first, then moves one lot of the buffer into the auction's vault. The engine refuses while debt is unbacked or the buffer is below its target plus a lot."),
                    confirmLabel: qsTr("Start surplus auction")
                }
            },
            submit: function() { return { action: "start_surplus", caller: caller } }
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
                spacing: Theme.spacing.medium
                LogosText {
                    objectName: "surplusTitle"
                    text: qsTr("Surplus auction")
                    font.pixelSize: Theme.typography.panelTitleText
                    font.weight: Theme.typography.weightMedium
                    color: Theme.palette.text
                }
                LogosBadge {
                    objectName: "surplusBadge"
                    visible: !!root.acc
                    text: root.live ? qsTr("Live") : qsTr("Idle")
                    color: root.live ? Theme.palette.accentOrange : Theme.palette.textSecondary
                }
            }
            LogosText {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: qsTr("Sells a lot of the buffer's stable for protocol tokens, which are burned.")
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textSecondary
            }

            InsetCard {
                Layout.fillWidth: true
                GridLayout {
                    Layout.fillWidth: true
                    columns: 4
                    columnSpacing: Theme.spacing.large
                    rowSpacing: Theme.spacing.small
                    uniformCellWidths: true
                    Fact {
                        objectName: "surplusBuffer"
                        Layout.fillWidth: true
                        label: qsTr("Buffer")
                        value: root.acc ? String(root.acc.buffer) : ""
                        decimals: 0
                        unit: qsTr("stable")
                    }
                    Fact {
                        objectName: "surplusLotLeft"
                        Layout.fillWidth: true
                        label: qsTr("Lot left")
                        value: root.live && root.acc.surplus_lot_left !== null && root.acc.surplus_lot_left !== undefined
                               ? String(root.acc.surplus_lot_left) : "—"
                        decimals: root.live ? 0 : -1
                        unit: root.live ? qsTr("stable") : ""
                    }
                    Fact {
                        objectName: "surplusNonce"
                        Layout.fillWidth: true
                        label: qsTr("Next auction nonce")
                        value: root.acc ? String(root.acc.surplus_nonce) : ""
                    }
                    Fact {
                        Layout.fillWidth: true
                        label: qsTr("Paid in")
                        value: qsTr("protocol token, burned")
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.spacing.medium

                InsetCard {
                    objectName: "startSurplusCard"
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1
                    Layout.alignment: Qt.AlignTop
                    spacing: Theme.spacing.small

                    LogosText {
                        text: qsTr("Start an auction")
                        font.pixelSize: Theme.typography.primaryText
                        font.weight: Theme.typography.weightBold
                        color: Theme.palette.text
                    }
                    LogosText {
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        text: qsTr("Anyone may start one. The engine starts it only when no debt is unbacked and the buffer holds its target plus a lot.")
                        font.pixelSize: Theme.typography.secondaryText
                        color: Theme.palette.textSecondary
                    }
                    ActionButton {
                        objectName: "startSurplus"
                        primary: true
                        Layout.fillWidth: true
                        text: qsTr("Start surplus auction")
                        enabled: root.canStart
                        onClicked: root.start()
                    }
                    LogosText {
                        objectName: "startSurplusReason"
                        Layout.fillWidth: true
                        visible: text.length > 0
                        wrapMode: Text.WordWrap
                        horizontalAlignment: Text.AlignHCenter
                        text: !root.acc ? ""
                              : root.live ? qsTr("A surplus auction is live: take it, or close it after its TTL, first.")
                              : !root.hasSigner ? qsTr("Pick an account in the header to sign.")
                              : ""
                        font.pixelSize: Theme.typography.secondaryText
                        color: Theme.palette.textSecondary
                    }
                }

                TakeCard {
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1
                    Layout.alignment: Qt.AlignTop
                    visible: root.live
                    page: root.page
                    overview: root.overview
                    backend: root.backend
                    surplus: true
                    noteName: "takeSurplusBurnNote"
                }
            }
        }
    }
}
