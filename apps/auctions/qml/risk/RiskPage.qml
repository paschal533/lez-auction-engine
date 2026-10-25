import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"

// The At-risk positions tab (board v4): the scanned positions on the left (lowest CR first), the selected
// position's panel on the right. There is no overview poll (R28): the screen reads `view at_risk`, the selected
// `position` (its health, liquidation quote or refusal) and `overview` (for the oracle) when it is shown, after
// a transaction lane reaches done, and after a rescan. Rescan (`riskRescan`) pages the module `rescan` until
// `done`, one page at a time; a page reporting `restarted` (the localnet was replaced, Review Focus 5) empties
// the list at once instead of keeping the old chain's positions.
Item {
    id: root
    objectName: "riskPage"

    property var backend: null
    property var accounts: []
    property string activeAccount: ""

    readonly property bool rescanning: d.rescanning
    readonly property alias items: d.items

    // Every overview this screen reads, for the header (Main.lastOverview).
    signal overviewRead(var overview)

    function refresh() { d.refresh() }
    function rescan() { d.rescanPage(true) }

    onVisibleChanged: if (visible) d.refresh()
    // A hidden tab reads nothing until it is shown.
    Component.onCompleted: if (visible) d.refresh()

    QtObject {
        id: d

        property var items: []
        property int scannedTo: -1
        property string selectedId: ""
        property var position: null
        // The whole overview reply (its oracle, and the block the quote header names), or null.
        property var overview: null
        readonly property var oracle: overview && overview.oracle ? overview.oracle : null
        property var listNotice: null
        property string listPrefix: ""
        property var detailNotice: null
        property int generation: 0
        property bool rescanning: false
        property var lastPage: null

        function failure(reply, err) {
            if (err !== undefined) return { error: String(err), kind: "ipc" }
            if (!reply || typeof reply !== "object") return { error: qsTr("The engine sent no reply."), kind: "bad_reply" }
            return reply.error ? reply : null
        }

        function setListNotice(prefix, notice) {
            listPrefix = prefix
            listNotice = notice
        }

        // A refresh while a rescan pages is skipped: the rescan refreshes once it is done, and a reply started
        // before a `restarted` page would bring back the replaced chain's positions.
        function refresh() {
            if (!root.backend || rescanning)
                return
            const gen = ++generation
            logos.watch(root.backend.view({ view: "at_risk" }), function(reply) {
                if (gen !== generation) return
                const bad = failure(reply)
                if (bad) {
                    setListNotice(qsTr("At-risk positions"), bad)
                    return
                }
                setListNotice("", null)
                items = Array.isArray(reply.items) ? reply.items : []
                scannedTo = reply.scanned_to !== undefined && reply.scanned_to !== null ? Number(reply.scanned_to) : -1
                const sorted = list.sortItems(items)
                if (!sorted.some(function(it) { return it.position === selectedId }))
                    selectedId = sorted.length > 0 ? String(sorted[0].position) : ""
                loadDetail()
            }, function(err) {
                if (gen === generation) setListNotice(qsTr("At-risk positions"), failure(null, err))
            })
            loadOracle(gen)
        }

        function select(id) {
            if (id === selectedId)
                return
            selectedId = id
            position = null
            loadDetail()
        }

        function loadDetail() {
            const id = selectedId
            if (!root.backend || id.length === 0) {
                position = null
                detailNotice = null
                return
            }
            logos.watch(root.backend.view({ view: "position", id: id }), function(reply) {
                if (id !== selectedId) return
                const bad = failure(reply)
                if (bad) {
                    detailNotice = bad
                    return
                }
                detailNotice = null
                position = reply
            }, function(err) {
                if (id === selectedId) detailNotice = failure(null, err)
            })
        }

        // The overview: its oracle for the pause states and its block for the quote header. A failed read
        // leaves it unknown (null): the quote card then says "Oracle unknown" and does not liquidate.
        function loadOracle(gen) {
            logos.watch(root.backend.view({ view: "overview" }), function(reply) {
                if (gen !== generation) return
                overview = failure(reply) ? null : reply
                if (overview) root.overviewRead(overview)
            }, function(err) {
                if (gen === generation) overview = null
            })
        }

        // A replaced chain: drop the list and every reply still on its way for the old one.
        function clearList() {
            ++generation
            items = []
            selectedId = ""
            position = null
            detailNotice = null
        }

        // One rescan page; the next is asked for only after this one answers (no overlapping rescans).
        function rescanPage(first) {
            if (!root.backend)
                return
            if (first) {
                if (rescanning)
                    return
                rescanning = true
                lastPage = null
                setListNotice("", null)
            }
            logos.watch(root.backend.rescan({}), function(reply) {
                const bad = failure(reply)
                if (bad) {
                    rescanning = false
                    setListNotice(qsTr("Rescan"), bad)
                    return
                }
                lastPage = reply
                if (reply.restarted)
                    clearList()
                if (reply.done) {
                    rescanning = false
                    refresh()
                    return
                }
                rescanPage(false)
            }, function(err) {
                rescanning = false
                setListNotice(qsTr("Rescan"), failure(null, err))
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

            PositionList {
                id: list
                Layout.fillWidth: true
                Layout.fillHeight: true
                items: d.items
                selectedId: d.selectedId
                scannedTo: d.scannedTo
                rescanning: d.rescanning
                lastPage: d.lastPage
                onPicked: function(id) { d.select(id) }
                onRescanRequested: root.rescan()
            }

            StateNotice {
                objectName: "riskNotice"
                Layout.fillWidth: true
                prefix: d.listPrefix
                reply: d.listNotice
            }

            LogosText {
                objectName: "riskExplainer"
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: qsTr("Positions are found by scanning blocks; there is no indexer yet. Rescan picks up new positions and price changes.")
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textSecondary
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: Theme.spacing.small

            StateNotice {
                objectName: "positionNotice"
                Layout.fillWidth: true
                prefix: qsTr("Position")
                reply: d.detailNotice
            }

            PositionDetail {
                Layout.fillWidth: true
                Layout.fillHeight: true
                backend: root.backend
                position: d.position
                oracle: d.oracle
                block: d.overview && d.overview.block !== undefined && d.overview.block !== null
                       ? Number(d.overview.block) : -1
                accounts: root.accounts
                activeAccount: root.activeAccount
                onLaneDone: d.refresh()
            }
        }
    }
}
