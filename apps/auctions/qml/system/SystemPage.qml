import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../shell"
import "../common/Outcome.js" as Outcome

// The System tab (board v5): Accounting / Surplus auction / Debt auction / Host on the left, the selected
// one's panel on the right. Every figure is a field of `view overview` (accounting, host) or of a quote view;
// nothing is predicted. There is no poll (R28): the overview is read when the tab is shown, on Refresh
// (`systemRefresh`), and after a transaction lane reaches done; each read goes to overviewRead() for the
// header's Frozen badge (Main.lastOverview).
// Transactions share one flow and one dialog / lane pair, prefixed "system" (R34, R38): a panel calls act(spec)
//   spec.title      what failed, for the notice ("Settle debt")
//   spec.read       the view read first: a quote (quote_settle_debt, quote_take_*) or `overview` for actions
//                   without one (a fresh overview also replaces the panel's figures)
//   spec.check(r)   optional: false drops the action after the read (the read already shows why)
//   spec.dialog(r)  the TxDialog request built from the read
//   spec.submit(r)  the auction_ffi prepare request (crates/auction_ffi/src/prepare.rs Action)
// then TxDialog → submit → TxLane. A failed read or submit (kind `refused` for an engine or host refusal,
// `mismatch` for the wrong network, R22) shows in `systemActionNotice`.
Item {
    id: root
    objectName: "systemPage"

    property var backend: null
    property var accounts: []
    property string activeAccount: ""
    // The section Main asks for ("accounting" from a Settled shortfall); empty keeps the current one.
    property string section: ""
    // The clock the relist delay (ADR-039) is judged by, in ms; tests pass a fake one.
    property var clock: function() { return Date.now() }

    readonly property alias selected: d.selected
    readonly property alias overview: d.overview
    // Busy from the read to the lane's outcome: reading, a submit in flight (no reply yet), or the lane waiting.
    readonly property bool busy: d.reading || d.submitting || (lane.txId.length > 0 && lane.state === "waiting")
    // The active account's wallet row, or null: the signer of every System transaction.
    readonly property var signer: {
        const rows = accounts || []
        for (let i = 0; i < rows.length; i++)
            if (rows[i] && !rows[i].error && rows[i].label === activeAccount) return rows[i]
        return null
    }

    signal overviewRead(var overview)

    function refresh() { d.refresh() }
    function select(name) {
        if (["accounting", "surplus", "debt", "host"].indexOf(name) >= 0)
            d.selected = name
    }
    function act(spec) { d.act(spec) }

    onSectionChanged: select(section)
    onVisibleChanged: if (visible) d.refresh()
    Component.onCompleted: {
        select(section)
        if (visible) d.refresh()
    }

    QtObject {
        id: d

        property string selected: "accounting"
        property var overview: null
        property var notice: null
        property int generation: 0
        // A transaction being prepared (read → dialog → submit), and its read in flight.
        property var pending: null
        property bool reading: false
        // A submit sent and not yet answered: a second Confirm or act() must not send another transaction.
        property bool submitting: false
        // The active account when the pending action started: the details build their dialog and submit for it.
        property string pendingAccount: ""
        property string pendingFrom: ""
        property var actionNotice: null
        property string actionPrefix: ""

        function failure(reply, err) {
            if (err !== undefined) return { error: String(err), kind: "ipc" }
            if (!reply || typeof reply !== "object") return { error: qsTr("The engine sent no reply."), kind: "bad_reply" }
            return reply.error ? reply : null
        }

        function adopt(reply) {
            overview = reply
            notice = null
            root.overviewRead(reply)
        }

        function refresh() {
            if (!root.backend)
                return
            const gen = ++generation
            logos.watch(root.backend.view({ view: "overview" }), function(reply) {
                if (gen !== generation) return
                const bad = failure(reply)
                if (bad) {
                    notice = bad
                    return
                }
                adopt(reply)
            }, function(err) {
                if (gen === generation) notice = failure(null, err)
            })
        }

        function setActionNotice(prefix, bad) {
            actionPrefix = prefix
            actionNotice = bad
        }

        function act(spec) {
            if (!root.backend || root.busy || !spec)
                return
            setActionNotice("", null)
            // A finished lane makes room; an unknown outcome stays on screen until the next submit replaces it.
            if (lane.state === "done" || lane.state === "dropped")
                lane.txId = ""
            pending = spec
            pendingAccount = root.activeAccount
            pendingFrom = root.signer ? String(root.signer.id) : ""
            reading = true
            logos.watch(root.backend.view(spec.read), function(reply) {
                if (pending !== spec) return
                reading = false
                const bad = failure(reply)
                if (bad) {
                    setActionNotice(spec.title, bad)
                    return
                }
                if (spec.read.view === "overview") {
                    ++generation
                    adopt(reply)
                }
                if (spec.check && !spec.check(reply))
                    return
                dialog.quote = reply
                dialog.request = spec.dialog(reply)
                dialog.open()
            }, function(err) {
                if (pending !== spec) return
                reading = false
                setActionNotice(spec.title, failure(null, err))
            })
        }

        function submit() {
            const spec = pending
            if (!root.backend || !spec || submitting)
                return
            // The dialog named the account the action started with; another one now would sign instead.
            const changed = Outcome.signerChanged(pendingAccount, root.activeAccount)
            if (changed) {
                setActionNotice(spec.title, changed)
                return
            }
            submitting = true
            const from = pendingFrom
            logos.watch(root.backend.submit(spec.submit(dialog.quote)), function(reply) {
                submitting = false
                const bad = failure(reply)
                if (bad) {
                    setActionNotice(spec.title, bad)
                    return
                }
                laneTitle.text = dialog.titleText
                lane.submitReply = reply
                lane.request = { from: from }
                lane.txId = String(reply.txId)
            }, function(err) {
                submitting = false
                setActionNotice(spec.title, failure(null, err))
            })
        }
    }

    RowLayout {
        anchors.fill: parent
        spacing: Theme.spacing.xlarge

        ColumnLayout {
            Layout.preferredWidth: 200
            Layout.maximumWidth: 200
            Layout.fillHeight: true
            spacing: Theme.spacing.medium

            SystemList {
                Layout.fillWidth: true
                overview: d.overview
                selected: d.selected
                onPicked: function(name) { root.select(name) }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.spacing.tiny
                LogosText {
                    objectName: "systemBlock"
                    Layout.fillWidth: true
                    text: d.overview && d.overview.block !== undefined && d.overview.block !== null
                          ? qsTr("Read at block %1").arg(d.overview.block) : ""
                    font.pixelSize: Theme.typography.secondaryText
                    font.features: { "tnum": 1 }
                    color: Theme.palette.textTertiary
                    elide: Text.ElideRight
                }
                LogosLink {
                    objectName: "systemRefresh"
                    text: qsTr("Refresh")
                    underline: false
                    font.pixelSize: Theme.typography.secondaryText
                    onActivated: d.refresh()
                }
            }

            StateNotice {
                objectName: "systemNotice"
                Layout.fillWidth: true
                prefix: qsTr("System")
                reply: d.notice
            }

            LogosText {
                objectName: "systemExplainer"
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: qsTr("Figures are read from the chain when this tab opens and after every included transaction, never predicted.")
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textSecondary
            }

            Item { Layout.fillHeight: true }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: Theme.spacing.small

            StateNotice {
                objectName: "systemActionNotice"
                Layout.fillWidth: true
                prefix: d.actionPrefix
                reply: d.actionNotice
            }

            InsetCard {
                objectName: "systemLaneCard"
                Layout.fillWidth: true
                visible: lane.txId.length > 0
                spacing: Theme.spacing.small

                LogosText {
                    id: laneTitle
                    objectName: "systemLaneTitle"
                    font.pixelSize: Theme.typography.primaryText
                    font.weight: Theme.typography.weightBold
                    color: Theme.palette.text
                }
                TxLane {
                    id: lane
                    namePrefix: "system"
                    Layout.fillWidth: true
                    backend: root.backend
                    accounts: root.accounts
                    onStateChanged: if (state === "done") d.refresh()
                    onRequote: if (d.pending) d.act(d.pending)
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.fillHeight: true
                color: Theme.palette.surfaceRaised
                radius: Theme.spacing.radiusXlarge

                StackLayout {
                    anchors.fill: parent
                    anchors.margins: Theme.spacing.large
                    currentIndex: ["accounting", "surplus", "debt", "host"].indexOf(d.selected)

                    AccountingDetail {
                        page: root
                        backend: root.backend
                        overview: d.overview
                    }
                    SurplusDetail {
                        page: root
                        backend: root.backend
                        overview: d.overview
                    }
                    DebtDetail {
                        page: root
                        overview: d.overview
                        clock: root.clock
                    }
                    HostDetail {
                        overview: d.overview
                    }
                }
            }
        }
    }

    TxDialog {
        id: dialog
        namePrefix: "system"
        onConfirmed: d.submit()
    }
}
