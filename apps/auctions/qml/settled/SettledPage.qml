import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format

// The Settled tab (board v5): the settled auctions on the left, the selected one's panel on the right. There is
// no poll (R28): the screen reads `view settled` and the selected `auction` when it is shown, after a claim's
// lane reaches done, and after a rescan. Rescan (`settledRescan`) pages the module `rescan` until `done`, one
// page at a time; a page reporting `restarted` (the localnet was replaced, Review Focus 5) empties the list at
// once. systemRequested("accounting") asks Main to show the System tab's Accounting (an auction that ended short).
Item {
    id: root
    objectName: "settledPage"

    property var backend: null
    property var accounts: []
    property string activeAccount: ""

    readonly property bool rescanning: d.rescanning
    readonly property alias items: d.items

    signal systemRequested(string section)

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
        property var auction: null
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
        // before a `restarted` page would bring back the replaced chain's auctions.
        function refresh() {
            if (!root.backend || rescanning)
                return
            const gen = ++generation
            logos.watch(root.backend.view({ view: "settled" }), function(reply) {
                if (gen !== generation) return
                const bad = failure(reply)
                if (bad) {
                    setListNotice(qsTr("Settled auctions"), bad)
                    return
                }
                setListNotice("", null)
                items = Format.list(reply.items)
                scannedTo = reply.scanned_to !== undefined && reply.scanned_to !== null ? Number(reply.scanned_to) : -1
                if (!items.some(function(it) { return it.id === selectedId }))
                    selectedId = items.length > 0 ? String(items[0].id) : ""
                loadDetail()
            }, function(err) {
                if (gen === generation) setListNotice(qsTr("Settled auctions"), failure(null, err))
            })
        }

        function select(id) {
            if (id === selectedId)
                return
            selectedId = id
            auction = null
            loadDetail()
        }

        function loadDetail() {
            const id = selectedId
            if (!root.backend || id.length === 0) {
                auction = null
                detailNotice = null
                return
            }
            logos.watch(root.backend.view({ view: "auction", id: id }), function(reply) {
                if (id !== selectedId) return
                const bad = failure(reply)
                if (bad) {
                    detailNotice = bad
                    return
                }
                detailNotice = null
                auction = reply
            }, function(err) {
                if (id === selectedId) detailNotice = failure(null, err)
            })
        }

        // A fresh read the claim card made: it replaces the panel's figures when it is for the selected auction.
        function adopt(reply) {
            if (reply && reply.id === selectedId && !reply.error)
                auction = reply
        }

        // A replaced chain: drop the list and every reply still on its way for the old one.
        function clearList() {
            ++generation
            items = []
            selectedId = ""
            auction = null
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

            SettledList {
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
                objectName: "settledNotice"
                Layout.fillWidth: true
                prefix: d.listPrefix
                reply: d.listNotice
            }

            LogosText {
                objectName: "settledExplainer"
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: qsTr("An auction settles when its target is raised or its lot sells out. A shortfall becomes unbacked debt (see System). Listed in scan order: the chain records no settle time.")
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textSecondary
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: Theme.spacing.small

            StateNotice {
                objectName: "settledDetailNotice"
                Layout.fillWidth: true
                prefix: qsTr("Auction")
                reply: d.detailNotice
            }

            SettledDetail {
                Layout.fillWidth: true
                Layout.fillHeight: true
                backend: root.backend
                auction: d.auction
                accounts: root.accounts
                activeAccount: root.activeAccount
                onLaneDone: d.refresh()
                onSystemRequested: function(section) { root.systemRequested(section) }
                onAuctionRead: function(reply) { d.adopt(reply) }
            }
        }
    }
}
