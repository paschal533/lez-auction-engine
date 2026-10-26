import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format

// Accounting (board v5): the KPIs (buffer, unbacked debt, debt on auction, freeze threshold) from
// overview.accounting; "Can the buffer cover it?", two bars on one scale (the largest of the three) with the
// threshold tick; Upkeep, `observeFeed` and `checkSolvency` (Secondary, 32 px); and the Settle debt card. The
// card shows `quote_settle_debt` as returned: the engine's Accounting::settle_debt,
// min(amount, buffer, unbacked − debt on auction) (R38), read again whenever the overview changes.
// `settleDebtSubmit` reads the quote again, then confirms and submits {action: settle_debt, caller}.
Item {
    id: root
    objectName: "accountingDetail"

    // The SystemPage: act(spec), busy, signer, activeAccount.
    property var page: null
    property var backend: null
    property var overview: null

    readonly property var acc: overview && overview.accounting ? overview.accounting : null
    readonly property bool hasSigner: !!(page && page.signer)
    readonly property bool canAct: !!acc && hasSigner && !!page && !page.busy
    readonly property var quote: d.quote
    readonly property bool canSettle: canAct && !!quote && Format.cmp(String(quote.amount), "0") > 0

    // The largest of buffer, unbacked debt and threshold: the scale of the coverage bars.
    readonly property string scaleMax: {
        if (!acc) return "0"
        let m = String(acc.buffer)
        for (const v of [String(acc.unbacked_debt), String(acc.freeze_threshold)])
            if (Format.cmp(v, m) > 0) m = v
        return m
    }
    readonly property bool belowThreshold: !!acc && Format.cmp(String(acc.unbacked_debt), String(acc.freeze_threshold)) <= 0
    readonly property bool hostFrozen: !!(overview && overview.host && overview.host.frozen === true)
    // What check_solvency would decide (auction_core system.rs, programs.md §check_solvency): Healthy at or below
    // the threshold (equality included); above it Freeze only when no debt auction can ever start (token not
    // configured, or MintCapConfig / MintAuthorityMissing / ProtocolTokenMismatch), NoAction for a startable or
    // transiently failing start, and a refusal (RecoveryInProgress) while a debt auction is live. The QML cannot
    // tell those apart, so above the threshold it says "may".
    readonly property string solvencyVerdict: {
        if (!acc) return ""
        const now = amount(acc.unbacked_debt)
        if (belowThreshold)
            return hostFrozen
                ? qsTr("Now %1: at or below the threshold. Healthy: no new freeze; the host stays frozen until its freeze authority unfreezes it.").arg(now)
                : qsTr("Now %1: at or below the threshold. Healthy: no freeze.").arg(now)
        if (acc.debt_live)
            return qsTr("Now %1: above the threshold, and a debt auction is live: the check is refused.").arg(now)
        return qsTr("Now %1: above the threshold. The engine may freeze new debt (when no debt auction can ever start).").arg(now)
    }

    onOverviewChanged: d.loadQuote()

    function amount(v) { return Format.amount(String(v), 0) }

    // A label / value line of the settle card; the value is `settleRowValue`.
    component Line: RowLayout {
        id: line
        property string label: ""
        property string value: ""
        property bool strong: false

        Layout.fillWidth: true
        Layout.minimumHeight: 22
        spacing: Theme.spacing.small
        LogosText {
            Layout.fillWidth: true
            text: line.label
            font.pixelSize: Theme.typography.secondaryText
            color: line.strong ? Theme.palette.textSecondary : Theme.palette.textTertiary
            elide: Text.ElideRight
        }
        LogosText {
            objectName: "settleRowValue"
            text: line.value.length > 0 ? Format.amount(line.value, 0) : "—"
            font.pixelSize: Theme.typography.primaryText
            font.weight: line.strong ? Theme.typography.weightBold : Theme.typography.weightMedium
            font.features: { "tnum": 1 }
            color: Theme.palette.text
        }
        LogosText {
            text: qsTr("stable")
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textSecondary
        }
    }

    // One coverage bar: label and value over a track; the fill is the value's share of the scale.
    component CoverBar: ColumnLayout {
        id: bar
        property string name: ""
        property string label: ""
        property string value: "0"
        property color fill: Theme.palette.textTertiary

        Layout.fillWidth: true
        spacing: Theme.spacing.tiny
        RowLayout {
            Layout.fillWidth: true
            LogosText {
                Layout.fillWidth: true
                text: bar.label
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textTertiary
            }
            LogosText {
                objectName: "coverValue" + bar.name
                text: Format.amount(bar.value, 0)
                font.pixelSize: Theme.typography.primaryText
                font.weight: Theme.typography.weightBold
                font.features: { "tnum": 1 }
                color: Theme.palette.text
            }
        }
        Rectangle {
            objectName: "coverTrack" + bar.name
            Layout.fillWidth: true
            Layout.preferredHeight: 8
            radius: 4
            color: Theme.palette.backgroundButton
            Rectangle {
                objectName: "coverBar" + bar.name
                width: parent.width * Format.ratio(bar.value, root.scaleMax)
                height: parent.height
                radius: 4
                color: bar.fill
            }
        }
    }

    component CardTitle: RowLayout {
        id: title
        property string text: ""
        property string caption: ""
        property string captionName: ""
        Layout.fillWidth: true
        LogosText {
            Layout.fillWidth: true
            text: title.text
            font.pixelSize: Theme.typography.primaryText
            font.weight: Theme.typography.weightBold
            color: Theme.palette.text
        }
        LogosText {
            objectName: title.captionName
            visible: text.length > 0
            text: title.caption
            font.pixelSize: Theme.typography.secondaryText
            font.features: { "tnum": 1 }
            color: Theme.palette.textTertiary
        }
    }

    QtObject {
        id: d

        property var quote: null
        property var quoteNotice: null
        property int generation: 0

        function loadQuote() {
            // Not root.acc: this runs from onOverviewChanged, possibly before that binding has caught up.
            if (!root.backend || !root.overview || !root.overview.accounting)
                return
            const gen = ++generation
            logos.watch(root.backend.view({ view: "quote_settle_debt" }), function(reply) {
                if (gen !== generation) return
                if (!reply || reply.error) {
                    quote = null
                    quoteNotice = reply && reply.error ? reply : { error: qsTr("The engine sent no quote."), kind: "bad_reply" }
                    return
                }
                quoteNotice = null
                quote = reply
            }, function(err) {
                if (gen !== generation) return
                quote = null
                quoteNotice = { error: String(err), kind: "ipc" }
            })
        }

        // The signer is read when the action starts (as Debt and Surplus do): the dialog and the submit name it.
        function caller() { return { caller: root.page.activeAccount } }
        function from() { return root.page.signer ? String(root.page.signer.id) : "" }

        function observeFeed() {
            const caller = d.caller()
            const from = d.from()
            root.page.act({
                title: qsTr("Observe price feed"),
                read: { view: "overview" },
                dialog: function(o) {
                    return {
                        action: "observe_feed",
                        from: from,
                        facts: [{ label: qsTr("Caller"), value: caller.caller }],
                        note: qsTr("Records whether the price feed is valid now on the collateral's pause clock. It changes no price."),
                        confirmLabel: qsTr("Observe feed")
                    }
                },
                submit: function() { return Object.assign({ action: "observe_feed" }, caller) }
            })
        }

        function checkSolvency() {
            const caller = d.caller()
            const from = d.from()
            root.page.act({
                title: qsTr("Check solvency"),
                read: { view: "overview" },
                dialog: function(o) {
                    return {
                        action: "check_solvency",
                        from: from,
                        facts: [
                            { label: qsTr("Unbacked debt"), value: String(o.accounting.unbacked_debt), decimals: 0, unit: qsTr("stable") },
                            { label: qsTr("Freeze threshold"), value: String(o.accounting.freeze_threshold), decimals: 0, unit: qsTr("stable") }
                        ],
                        note: qsTr("Settles what the buffer can first. At or below the threshold that is all: no freeze. Above it the engine may freeze new debt (when no debt auction can ever start); while a debt auction is live the check is refused."),
                        confirmLabel: qsTr("Check solvency")
                    }
                },
                submit: function() { return Object.assign({ action: "check_solvency" }, caller) }
            })
        }

        function settleDebt() {
            const caller = d.caller()
            const from = d.from()
            root.page.act({
                title: qsTr("Settle debt"),
                read: { view: "quote_settle_debt" },
                check: function(q) {
                    d.quote = q
                    return Format.cmp(String(q.amount), "0") > 0
                },
                dialog: function(q) {
                    return {
                        action: "settle_debt",
                        from: from,
                        facts: [
                            { label: qsTr("Settles"), value: String(q.amount), decimals: 0, unit: qsTr("stable"), strong: true },
                            { label: qsTr("Unbacked after"), value: String(q.unbacked_after), decimals: 0, unit: qsTr("stable") },
                            { label: qsTr("Buffer after"), value: String(q.buffer_after), decimals: 0, unit: qsTr("stable") }
                        ],
                        note: qsTr("The buffer's stable is burned against the unbacked debt. The engine settles what the buffer holds when the transaction runs."),
                        confirmLabel: qsTr("Settle debt")
                    }
                },
                submit: function() { return Object.assign({ action: "settle_debt" }, caller) }
            })
        }
    }

    Flickable {
        id: flick
        objectName: "accountingScroll"
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

            LogosText {
                objectName: "accountingTitle"
                text: qsTr("Accounting")
                font.pixelSize: Theme.typography.panelTitleText
                font.weight: Theme.typography.weightMedium
                color: Theme.palette.text
            }

            KpiStrip {
                objectName: "accountingKpis"
                Layout.fillWidth: true
                items: !root.acc ? [] : [
                    { label: qsTr("Buffer"), value: String(root.acc.buffer), decimals: 0, unit: qsTr("stable"),
                      caption: qsTr("penalties collected") },
                    { label: qsTr("Unbacked debt"), value: String(root.acc.unbacked_debt), decimals: 0, unit: qsTr("stable"),
                      caption: qsTr("debt no collateral covers") },
                    { label: qsTr("Debt on auction"), value: String(root.acc.debt_on_auction), decimals: 0, unit: qsTr("stable"),
                      caption: root.acc.debt_live ? qsTr("a debt auction is live") : qsTr("no debt auction live") },
                    { label: qsTr("Freeze threshold"), value: String(root.acc.freeze_threshold), decimals: 0, unit: qsTr("stable"),
                      caption: qsTr("unbacked debt above this") }
                ]
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.spacing.medium

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1
                    Layout.alignment: Qt.AlignTop
                    spacing: Theme.spacing.medium

                    // ---- Can the buffer cover it? ------------------------------------------------------
                    InsetCard {
                        objectName: "coverCard"
                        Layout.fillWidth: true
                        spacing: Theme.spacing.small

                        CardTitle {
                            text: qsTr("Can the buffer cover it?")
                            caption: root.acc ? qsTr("0 – %1 stable").arg(root.amount(root.scaleMax)) : ""
                            captionName: "coverScale"
                        }
                        Item {
                            objectName: "coverTicks"
                            Layout.fillWidth: true
                            implicitHeight: bars.implicitHeight

                            ColumnLayout {
                                id: bars
                                width: parent.width
                                spacing: Theme.spacing.small
                                CoverBar {
                                    name: "Buffer"
                                    label: qsTr("Buffer")
                                    value: root.acc ? String(root.acc.buffer) : "0"
                                    fill: Theme.palette.textTertiary
                                }
                                CoverBar {
                                    name: "Unbacked"
                                    label: qsTr("Unbacked debt")
                                    value: root.acc ? String(root.acc.unbacked_debt) : "0"
                                    fill: Theme.palette.error
                                }
                            }
                            // The freeze threshold on the same scale, across both bars.
                            Rectangle {
                                objectName: "coverTick"
                                visible: !!root.acc
                                x: parent.width * (root.acc ? Format.ratio(String(root.acc.freeze_threshold), root.scaleMax) : 0) - width / 2
                                y: 0
                                width: 2
                                height: parent.height
                                color: Theme.palette.text
                            }
                        }
                        LogosText {
                            objectName: "coverThreshold"
                            Layout.fillWidth: true
                            text: root.acc ? qsTr("Tick: %1 freeze threshold").arg(root.amount(root.acc.freeze_threshold)) : ""
                            font.pixelSize: Theme.typography.secondaryText
                            font.features: { "tnum": 1 }
                            color: Theme.palette.textSecondary
                        }
                    }

                    // ---- Upkeep -------------------------------------------------------------------------
                    InsetCard {
                        objectName: "upkeepCard"
                        Layout.fillWidth: true
                        spacing: Theme.spacing.medium

                        CardTitle { text: qsTr("Upkeep") }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: Theme.spacing.medium
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 2
                                LogosText {
                                    text: qsTr("Observe feed")
                                    font.pixelSize: Theme.typography.secondaryText
                                    font.weight: Theme.typography.weightBold
                                    color: Theme.palette.text
                                }
                                LogosText {
                                    Layout.fillWidth: true
                                    wrapMode: Text.WordWrap
                                    text: qsTr("Records whether the price feed is valid now, so the pause clock is exact. It changes no price.")
                                    font.pixelSize: Theme.typography.secondaryText
                                    color: Theme.palette.textSecondary
                                }
                            }
                            ActionButton {
                                objectName: "observeFeed"
                                Layout.alignment: Qt.AlignVCenter
                                text: qsTr("Observe feed")
                                enabled: root.canAct
                                onClicked: d.observeFeed()
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: Theme.spacing.medium
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 2
                                LogosText {
                                    text: qsTr("Check solvency")
                                    font.pixelSize: Theme.typography.secondaryText
                                    font.weight: Theme.typography.weightBold
                                    color: Theme.palette.text
                                }
                                LogosText {
                                    Layout.fillWidth: true
                                    wrapMode: Text.WordWrap
                                    text: root.acc
                                          ? qsTr("Settles what the buffer can, then judges unbacked debt against %1 stable. Above it the engine may freeze new debt (when no debt auction can ever start); while a debt auction is live the check is refused.")
                                                .arg(root.amount(root.acc.freeze_threshold))
                                          : ""
                                    font.pixelSize: Theme.typography.secondaryText
                                    color: Theme.palette.textSecondary
                                }
                                LogosText {
                                    objectName: "solvencyVerdict"
                                    Layout.fillWidth: true
                                    wrapMode: Text.WordWrap
                                    text: root.solvencyVerdict
                                    font.pixelSize: Theme.typography.secondaryText
                                    font.features: { "tnum": 1 }
                                    color: root.belowThreshold ? Theme.palette.success : Theme.palette.error
                                }
                            }
                            ActionButton {
                                objectName: "checkSolvency"
                                Layout.alignment: Qt.AlignVCenter
                                text: qsTr("Check solvency")
                                enabled: root.canAct
                                onClicked: d.checkSolvency()
                            }
                        }
                    }
                }

                // ---- Settle debt ------------------------------------------------------------------------
                InsetCard {
                    objectName: "settleDebtCard"
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1
                    Layout.alignment: Qt.AlignTop
                    spacing: Theme.spacing.small

                    CardTitle {
                        text: qsTr("Settle debt")
                        caption: root.overview && root.quote ? qsTr("at block %1").arg(root.overview.block) : ""
                        captionName: "settleBlock"
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        Line {
                            objectName: "settleUnbacked"
                            label: qsTr("Unbacked debt")
                            value: root.quote ? String(root.quote.unbacked_before) : ""
                        }
                        Line {
                            objectName: "settleOnAuction"
                            label: qsTr("Of it on a debt auction")
                            value: root.quote ? String(root.quote.debt_on_auction) : ""
                        }
                        Line {
                            objectName: "settleBuffer"
                            label: qsTr("Buffer")
                            value: root.quote ? String(root.quote.buffer_before) : ""
                        }
                    }

                    Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.palette.borderSubtle }

                    RowLayout {
                        Layout.fillWidth: true
                        LogosText {
                            Layout.fillWidth: true
                            text: qsTr("Settles")
                            font.pixelSize: Theme.typography.secondaryText
                            color: Theme.palette.textSecondary
                        }
                        LogosText {
                            objectName: "settleDebtAmount"
                            text: root.quote ? root.amount(root.quote.amount) : "—"
                            font.pixelSize: Theme.typography.panelTitleText
                            font.weight: Theme.typography.weightBold
                            font.features: { "tnum": 1 }
                            color: Theme.palette.accentOrange
                        }
                        LogosText {
                            text: qsTr("stable")
                            font.pixelSize: Theme.typography.primaryText
                            color: Theme.palette.textSecondary
                        }
                    }
                    LogosText {
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        text: qsTr("The smaller of the buffer and the unbacked debt not on a debt auction. The buffer's stable is burned against the debt.")
                        font.pixelSize: Theme.typography.secondaryText
                        color: Theme.palette.textTertiary
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        Line {
                            objectName: "settleUnbackedAfter"
                            label: qsTr("Unbacked after")
                            value: root.quote ? String(root.quote.unbacked_after) : ""
                        }
                        Line {
                            objectName: "settleBufferAfter"
                            label: qsTr("Buffer after")
                            value: root.quote ? String(root.quote.buffer_after) : ""
                        }
                    }

                    StateNotice {
                        objectName: "settleQuoteNotice"
                        Layout.fillWidth: true
                        prefix: qsTr("Settle debt quote")
                        reply: d.quoteNotice
                    }

                    ActionButton {
                        objectName: "settleDebtSubmit"
                        primary: true
                        Layout.fillWidth: true
                        text: qsTr("Settle debt")
                        enabled: root.canSettle
                        onClicked: d.settleDebt()
                    }
                    LogosText {
                        objectName: "settleDebtReason"
                        Layout.fillWidth: true
                        visible: text.length > 0
                        wrapMode: Text.WordWrap
                        horizontalAlignment: Text.AlignHCenter
                        text: !root.quote ? ""
                              : !root.hasSigner ? qsTr("Pick an account in the header to sign.")
                              : Format.cmp(String(root.quote.amount), "0") <= 0
                                ? qsTr("Nothing to settle: the buffer is empty, or all unbacked debt is on a debt auction.")
                                : ""
                        font.pixelSize: Theme.typography.secondaryText
                        color: Theme.palette.textSecondary
                    }
                }
            }
        }
    }
}
