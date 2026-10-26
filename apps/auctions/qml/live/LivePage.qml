import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format

// The Live auctions tab (board v3): the list on the left (AppManagerView's 200px column), the selected
// auction's panel on the right. There is no overview poll (R28): the screen reads `view live`, the selected
// `auction` and `overview` (for the oracle) when it is shown, after a transaction lane reaches done, and after
// a rescan. Rescan (`liveRescan`) pages `rescan` until `done`, one page at a time; a page reporting `restarted`
// (the localnet was replaced, Review Focus 5) empties the list at once instead of keeping stale auctions.
Item {
    id: root
    objectName: "livePage"

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
    Component.onCompleted: d.refresh()

    QtObject {
        id: d

        property var items: []
        property string selectedId: ""
        property var auction: null
        property var oracle: null
        // {error, kind} with the prefix it is shown under; null when all is well.
        property var listNotice: null
        property string listPrefix: ""
        property var detailNotice: null
        property string detailPrefix: ""
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

        function setDetailNotice(prefix, notice) {
            detailPrefix = prefix
            detailNotice = notice
        }

        // A refresh while a rescan pages is skipped: the rescan refreshes once it is done, and a reply
        // started before a `restarted` page would bring back the replaced chain's auctions.
        function refresh() {
            if (!root.backend || rescanning)
                return
            const gen = ++generation
            logos.watch(root.backend.view({ view: "live" }), function(reply) {
                if (gen !== generation) return
                const bad = failure(reply)
                if (bad) {
                    setListNotice(qsTr("Live auctions"), bad)
                    return
                }
                setListNotice("", null)
                items = Format.list(reply.items)
                const sorted = list.sortItems(items)
                if (!sorted.some(function(it) { return it.id === selectedId }))
                    selectedId = sorted.length > 0 ? String(sorted[0].id) : ""
                loadDetail()
            }, function(err) {
                if (gen === generation) setListNotice(qsTr("Live auctions"), failure(null, err))
            })
            loadOracle(gen)
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
                return
            }
            logos.watch(root.backend.view({ view: "auction", id: id }), function(reply) {
                if (id !== selectedId) return
                const bad = failure(reply)
                if (bad) {
                    setDetailNotice(qsTr("Auction"), bad)
                    return
                }
                setDetailNotice("", null)
                auction = reply
            }, function(err) {
                if (id === selectedId) setDetailNotice(qsTr("Auction"), failure(null, err))
            })
        }

        // The oracle for the bid card. A failed read leaves it unknown (null): the card then says
        // "Oracle unknown" and does not bid.
        function loadOracle(gen) {
            logos.watch(root.backend.view({ view: "overview" }), function(reply) {
                if (gen !== generation) return
                const bad = failure(reply)
                oracle = bad ? null : (reply.oracle || null)
                if (!bad) root.overviewRead(reply)
            }, function(err) {
                if (gen === generation) oracle = null
            })
        }

        // A replaced chain: drop the list and every reply still on its way for the old one.
        function clearList() {
            ++generation
            items = []
            selectedId = ""
            auction = null
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

            AuctionList {
                id: list
                Layout.fillWidth: true
                Layout.fillHeight: true
                items: d.items
                selectedId: d.selectedId
                onPicked: function(id) { d.select(id) }
            }

            StateNotice {
                objectName: "liveNotice"
                Layout.fillWidth: true
                prefix: d.listPrefix
                reply: d.listNotice
            }

            LogosText {
                objectName: "liveExplainer"
                Layout.fillWidth: true
                visible: text.length > 0
                wrapMode: Text.WordWrap
                text: {
                    const a = d.auction
                    // The schedule and its [t, ray] points may arrive as array-likes (R43).
                    const schedule = a ? Format.list(a.schedule).map(Format.list) : []
                    if (schedule.length === 0) return ""
                    const first = schedule[0][1]
                    let max = first
                    for (let i = 0; i < schedule.length; i++)
                        if (Format.cmp(String(schedule[i][1]), String(max)) > 0) max = schedule[i][1]
                    const minutes = Math.round((Number(a.max_at_ms) - Number(a.started_at_ms)) / 60000)
                    return qsTr("The discount starts at %1% and climbs to %2% over %3 min, then holds there with no time limit.")
                        .arg(Format.rayPercent(String(first), 1)).arg(Format.rayPercent(String(max), 1)).arg(minutes)
                }
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textSecondary
            }

            RowLayout {
                Layout.fillWidth: true
                LogosLink {
                    objectName: "liveRescan"
                    text: qsTr("Rescan")
                    underline: false
                    onActivated: root.rescan()
                }
                LogosText {
                    objectName: "liveRescanProgress"
                    Layout.fillWidth: true
                    visible: d.rescanning
                    text: d.lastPage
                          ? qsTr("block %1 of %2").arg(d.lastPage.scanned_to).arg(d.lastPage.head)
                          : qsTr("scanning…")
                    font.pixelSize: Theme.typography.secondaryText
                    font.features: { "tnum": 1 }
                    color: Theme.palette.textTertiary
                    elide: Text.ElideRight
                }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: Theme.spacing.small

            StateNotice {
                objectName: "detailNotice"
                Layout.fillWidth: true
                prefix: d.detailPrefix
                reply: d.detailNotice
            }

            AuctionDetail {
                Layout.fillWidth: true
                Layout.fillHeight: true
                backend: root.backend
                auction: d.auction
                oracle: d.oracle
                accounts: root.accounts
                activeAccount: root.activeAccount
                onLaneDone: d.refresh()
            }
        }
    }
}
