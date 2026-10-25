import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../shell"

// Dev tools (board v5): Oracle price / Token transfer / Protocol token on the left, the selected one's panel
// on the right. They replace the demo's terminal steps: the price move, the stable transfer before a debt take
// and the freeze trigger. Only on a verified dev network (`devNetwork`, deployment.dev: a named dev network
// whose block 1 matches the deployment's genesis hash); Main hides the tab otherwise, and off one this page
// reads nothing and offers no action (`devOffNotice`). The engine refuses a dev action on any other network
// with kind `mismatch` (R22), shown in `devActionNotice`.
// The overview (oracle price, age, range; host min CR; accounting) is read when the page is shown, on Refresh
// and after a lane reaches done; nothing polls (R28). Price and protocol-token transactions share one flow and
// one dialog / lane pair prefixed "dev" (R34): a panel calls act(spec), as on the System page:
//   spec.title       what failed, for the notice
//   spec.read        the view read first (quote_price, or overview)
//   spec.dialog(r)   the TxDialog request built from the read
//   spec.submit(r)   the auction_ffi prepare request (crates/auction_ffi/src/prepare.rs Action)
//   spec.stepLabels  optional: one lane label per step (oracle_publish has three, R27)
// The token transfer keeps its own form and status line (Task 5; skeleton.mjs reads `devTransferStatus`).
Item {
    id: root

    property var backend: null
    property var accounts: []
    property string activeAccount: ""
    property bool devNetwork: false
    property string network: ""
    // The panel to show ("price", "transfer", "token"); empty keeps the current one.
    property string section: ""

    readonly property bool active: devNetwork
    readonly property alias selected: d.selected
    readonly property alias overview: d.overview
    readonly property bool busy: d.reading || d.submitting || (lane.txId.length > 0 && lane.state === "waiting")
    // The active account's wallet row, or null: the host admin of a price publish.
    readonly property var signer: {
        const rows = accounts || []
        for (let i = 0; i < rows.length; i++)
            if (rows[i] && !rows[i].error && rows[i].label === activeAccount) return rows[i]
        return null
    }

    signal overviewRead(var overview)

    function refresh() { d.refresh() }
    function select(name) {
        if (["price", "transfer", "token"].indexOf(name) >= 0)
            d.selected = name
    }
    function act(spec) { d.act(spec) }

    onSectionChanged: select(section)
    onVisibleChanged: if (visible) d.refresh()
    onActiveChanged: if (active && visible) d.refresh()
    Component.onCompleted: {
        select(section)
        if (visible) d.refresh()
    }

    QtObject {
        id: d

        property string selected: "price"
        property var overview: null
        property var notice: null
        property int generation: 0
        property var pending: null
        property bool reading: false
        property bool submitting: false
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
            if (!root.backend || !root.active)
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
            if (!root.backend || !root.active || root.busy || !spec)
                return
            setActionNotice("", null)
            // A finished lane makes room; an unknown outcome stays on screen until the next submit replaces it.
            if (lane.state === "done" || lane.state === "dropped")
                lane.txId = ""
            pending = spec
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
            submitting = true
            logos.watch(root.backend.submit(spec.submit(dialog.quote)), function(reply) {
                submitting = false
                const bad = failure(reply)
                if (bad) {
                    setActionNotice(spec.title, bad)
                    return
                }
                laneTitle.text = dialog.titleText
                lane.stepLabels = spec.stepLabels || []
                lane.submitReply = reply
                lane.request = { from: root.signer ? String(root.signer.id) : "" }
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

            DevList {
                Layout.fillWidth: true
                overview: d.overview
                network: root.network
                selected: d.selected
                onPicked: function(name) { root.select(name) }
            }

            LogosLink {
                objectName: "devRefresh"
                text: qsTr("Refresh")
                underline: false
                enabled: root.active
                font.pixelSize: Theme.typography.secondaryText
                onActivated: d.refresh()
            }

            StateNotice {
                objectName: "devNotice"
                Layout.fillWidth: true
                prefix: qsTr("Dev tools")
                reply: d.notice
            }

            Item { Layout.fillHeight: true }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: Theme.spacing.small

            LogosNotice {
                objectName: "devOffNotice"
                Layout.fillWidth: true
                severity: LogosNotice.Warning
                message: qsTr("Dev tools work only on a verified dev network (localnet or testnet-legacy whose block 1 matches the deployment).")
                shown: !root.active
            }

            StateNotice {
                objectName: "devActionNotice"
                Layout.fillWidth: true
                prefix: d.actionPrefix
                reply: d.actionNotice
            }

            InsetCard {
                objectName: "devLaneCard"
                Layout.fillWidth: true
                visible: lane.txId.length > 0
                spacing: Theme.spacing.small

                LogosText {
                    id: laneTitle
                    objectName: "devLaneTitle"
                    font.pixelSize: Theme.typography.primaryText
                    font.weight: Theme.typography.weightBold
                    color: Theme.palette.text
                }
                TxLane {
                    id: lane
                    namePrefix: "dev"
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
                    currentIndex: ["price", "transfer", "token"].indexOf(d.selected)

                    OraclePriceDetail {
                        page: root
                        overview: d.overview
                    }
                    TransferDetail {
                        backend: root.backend
                        active: root.active
                    }
                    ProtocolTokenDetail {
                        page: root
                        overview: d.overview
                    }
                }
            }
        }
    }

    TxDialog {
        id: dialog
        namePrefix: "dev"
        onConfirmed: d.submit()
    }
}
