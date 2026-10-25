import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format

// Debt auction (board v5): mints protocol tokens to whoever pays down a lot of unbacked debt; the stable paid
// is burned. Facts from overview.accounting. `startDebt` is refused, with the SDK's reasons
// (ops/system.rs prepare_start_debt), while one is live and until `earliest_debt_start_ms`: the fixed relist
// delay after the previous debt auction ended (ADR-039, DEBT_RELIST_DELAY_MS = 600 000). The button stays
// disabled with that reason (`startDebtReason`) and a one-shot local timer (`relistTimer`) re-reads `clock`
// when the time passes; nothing polls the chain (R28). While one is live, the take card (`takeDebtAmount`,
// `takeDebtSubmit`, `takeDebtNote`).
Item {
    id: root
    objectName: "debtDetail"

    property var page: null
    property var overview: null
    // ms since the epoch, as Date.now(); the SDK judges the delay by the chain's clock (CLOCK_01).
    property var clock: function() { return Date.now() }

    readonly property var acc: overview && overview.accounting ? overview.accounting : null
    readonly property bool live: !!acc && acc.debt_live === true
    readonly property bool hasSigner: !!(page && page.signer)
    readonly property real earliest: acc && acc.earliest_debt_start_ms !== null && acc.earliest_debt_start_ms !== undefined
                                     ? Number(acc.earliest_debt_start_ms) : -1
    property real nowMs: clock()
    readonly property bool delayed: earliest >= 0 && nowMs < earliest
    readonly property bool canStart: !!acc && !live && !delayed && hasSigner && !page.busy

    // A new overview (or clock) is judged at the time it arrives.
    onOverviewChanged: nowMs = clock()
    onClockChanged: nowMs = clock()

    function start() {
        const caller = page.activeAccount
        const from = page.signer ? String(page.signer.id) : ""
        page.act({
            title: qsTr("Start debt auction"),
            read: { view: "overview" },
            check: function(o) { return !!o.accounting && !o.accounting.debt_live },
            dialog: function(o) {
                return {
                    action: "start_debt",
                    from: from,
                    facts: [
                        { label: qsTr("Unbacked debt"), value: String(o.accounting.unbacked_debt), decimals: 0, unit: qsTr("stable") },
                        { label: qsTr("Buffer"), value: String(o.accounting.buffer), decimals: 0, unit: qsTr("stable") },
                        { label: qsTr("Auction nonce"), value: String(o.accounting.debt_nonce) }
                    ],
                    note: qsTr("Settles what the buffer can first, then puts one lot of unbacked debt on auction. The engine refuses unless at least a lot is still unbacked."),
                    confirmLabel: qsTr("Start debt auction")
                }
            },
            submit: function() { return { action: "start_debt", caller: caller } }
        })
    }

    Timer {
        id: relistTimer
        objectName: "relistTimer"
        repeat: false
        // Timer intervals are a signed 32-bit count of ms.
        interval: Math.max(0, Math.min(2147483647, root.earliest - root.nowMs))
        running: root.delayed && !root.live
        onTriggered: root.nowMs = root.clock()
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
                    objectName: "debtTitle"
                    text: qsTr("Debt auction")
                    font.pixelSize: Theme.typography.panelTitleText
                    font.weight: Theme.typography.weightMedium
                    color: Theme.palette.text
                }
                LogosBadge {
                    objectName: "debtBadge"
                    visible: !!root.acc
                    text: root.live ? qsTr("Live") : qsTr("Idle")
                    color: root.live ? Theme.palette.accentOrange : Theme.palette.textSecondary
                }
            }
            LogosText {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: qsTr("Mints protocol tokens to whoever pays down a lot of unbacked debt. The stable paid is burned.")
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
                        objectName: "debtUnbacked"
                        Layout.fillWidth: true
                        label: qsTr("Unbacked debt")
                        value: root.acc ? String(root.acc.unbacked_debt) : ""
                        decimals: 0
                        unit: qsTr("stable")
                    }
                    Fact {
                        objectName: "debtOnAuction"
                        Layout.fillWidth: true
                        label: qsTr("On auction")
                        value: root.acc ? String(root.acc.debt_on_auction) : ""
                        decimals: 0
                        unit: qsTr("stable")
                    }
                    Fact {
                        objectName: "debtLotLeft"
                        Layout.fillWidth: true
                        label: qsTr("Lot left")
                        value: root.live && root.acc.debt_lot_left !== null && root.acc.debt_lot_left !== undefined
                               ? String(root.acc.debt_lot_left) : "—"
                        decimals: root.live ? 0 : -1
                        unit: root.live ? qsTr("stable") : ""
                    }
                    Fact {
                        objectName: "debtBuffer"
                        Layout.fillWidth: true
                        label: qsTr("Buffer")
                        value: root.acc ? String(root.acc.buffer) : ""
                        decimals: 0
                        unit: qsTr("stable")
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.spacing.medium

                InsetCard {
                    objectName: "startDebtCard"
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
                        text: qsTr("Anyone may start one. The engine starts it only when, after the buffer settles what it can, at least a lot of debt is still unbacked.")
                        font.pixelSize: Theme.typography.secondaryText
                        color: Theme.palette.textSecondary
                    }
                    ActionButton {
                        objectName: "startDebt"
                        primary: true
                        Layout.fillWidth: true
                        text: qsTr("Start debt auction")
                        enabled: root.canStart
                        onClicked: root.start()
                    }
                    LogosText {
                        objectName: "startDebtReason"
                        Layout.fillWidth: true
                        visible: text.length > 0
                        wrapMode: Text.WordWrap
                        horizontalAlignment: Text.AlignHCenter
                        text: !root.acc ? ""
                              : root.live ? qsTr("A debt auction is live: take it, or close it after its TTL, first.")
                              : root.delayed
                                ? qsTr("Relist delay (ADR-039): the next debt auction may start at %1, 10 minutes after the last one ended.")
                                      .arg(Qt.formatTime(new Date(root.earliest), "hh:mm:ss"))
                              : !root.hasSigner ? qsTr("Pick an account in the header to sign.")
                              : ""
                        font.pixelSize: Theme.typography.secondaryText
                        font.features: { "tnum": 1 }
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
                    backend: root.page ? root.page.backend : null
                    surplus: false
                }
            }
        }
    }
}
