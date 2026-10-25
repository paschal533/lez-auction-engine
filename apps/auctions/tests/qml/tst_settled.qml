import QtQuick
import QtTest

import Logos.Controls

import "../../qml/settled"
import "../../qml/common/Format.js" as Format
import "Fixtures.js" as Fixtures

// The Settled screen (board v5) against a stub backend that answers with the real fixtures (`settled`,
// `auction_settled`: a settled auction with no bids and a full shortfall). Other outcomes are variants built
// in-test from the real JSON, with numbers that keep the engine's rules (crates/auction_core/src/auction.rs):
//   raised = debt_repaid + penalty; a bid pays debt first, then penalty;
//   shortfall = debt_principal − debt_repaid (unbacked debt; the unraised penalty is never booked);
//   leftover = the collateral left, only when the whole target was raised; claimed ≤ leftover.
// The fixture's target 660,000 is a 600,000 debt plus a 60,000 penalty.
TestCase {
    id: testCase
    name: "Settled"
    width: 1100
    height: 720
    visible: true
    when: windowShown

    readonly property string ownerId: "2o6Ae5oEDKRmmQy4qtm8YhLhe3rGtkuNjs57it4gGdm5"   // "owner2" in accounts.json

    property var settledReply: ({})
    property var auctionReply: ({})
    property var submitReply: ({})
    property var statusReply: ({})
    property var rescanQueue: []
    property var calls: []
    property var listCountAtRescan: []
    property var page: null

    // Theme keeps its settings in QSettings, which needs an application identity (Main.qml).
    function initTestCase() {
        Qt.application.organization = "auctions-qml-tests"
        Qt.application.domain = "auctions.test"
    }

    QtObject {
        id: backendStub
        property string network: "localnet"
        property bool devNetwork: true
        property string activeAccount: "keeper"
        property bool walletOpen: true
        property bool nodeReachable: true
        property string lastError: ""
        function view(req) { testCase.calls.push({ call: "view", req: req }); return { call: "view", req: req } }
        function accounts() { return { call: "accounts" } }
        function rescan(req) {
            const list = testCase.page ? findChild(testCase.page, "settledList") : null
            testCase.listCountAtRescan.push(list ? list.count : -1)
            testCase.calls.push({ call: "rescan", req: req })
            return { call: "rescan" }
        }
        function submit(req) { testCase.calls.push({ call: "submit", req: req }); return { call: "submit" } }
        function txStatus(id) { return { call: "txStatus", txId: id } }
    }

    QtObject {
        id: logos
        function module(name) { return backendStub }
        function watch(promise, ok, err) {
            if (!promise) return
            const copy = function(v) { return JSON.parse(JSON.stringify(v)) }
            switch (promise.call) {
            case "accounts": ok(Fixtures.load("accounts")); return
            case "view":
                switch (promise.req.view) {
                case "settled": ok(copy(testCase.settledReply)); return
                case "auction": ok(copy(testCase.auctionReply)); return
                }
                ok({ error: "unknown view", kind: "bad_request" })
                return
            case "rescan":
                ok(testCase.rescanQueue.length > 0 ? testCase.rescanQueue.shift() : Fixtures.load("scan_done"))
                return
            case "submit": ok(testCase.submitReply); return
            case "txStatus": ok(testCase.statusReply); return
            }
        }
    }

    Component {
        id: pageComponent
        SettledPage {
            width: 1020
            height: 600
            backend: backendStub
        }
    }

    function init() {
        settledReply = Fixtures.load("settled")
        auctionReply = Fixtures.load("auction_settled")
        submitReply = { txId: "tx1", hash: "h", what: "claim_leftover", step: 0, steps: 1,
                        tracking: { signer: ownerId } }
        statusReply = { txId: "tx1", state: "pending", blocks_waited: 1, step: 0, steps: 1 }
        rescanQueue = []
        calls = []
        listCountAtRescan = []
        page = null
        backendStub.activeAccount = "keeper"
    }

    function cleanup() {
        page = null
    }

    // ---- Variants of the real fixture (engine-consistent, see the header) -------------------------

    // Applied to the list row and the `auction` view alike.
    function variant(fields) {
        for (const k in fields) {
            settledReply.items[0][k] = fields[k]
            auctionReply[k] = fields[k]
        }
    }

    // The whole target raised with collateral to spare: debt 600,000 + penalty 60,000 = 660,000 raised,
    // 400,000 of the 475,000 lot sold, 75,000 left over for the owner.
    function targetRaised() {
        variant({ bids: 3, debt_repaid: "600000", penalty: "60000", raised: "660000", shortfall: "0",
                  sold: "400000", leftover: "75000", claimed: "0" })
    }

    // The lot sold out before the debt was covered: 450,000 repaid, no penalty, 150,000 short.
    function endedShort() {
        variant({ bids: 2, debt_repaid: "450000", penalty: "0", raised: "450000", shortfall: "150000",
                  sold: "475000", leftover: "0", claimed: "0" })
    }

    // The lot sold out after the debt but before the whole penalty: nothing short, nothing left over.
    function soldOut() {
        variant({ bids: 4, debt_repaid: "600000", penalty: "30000", raised: "630000", shortfall: "0",
                  sold: "475000", leftover: "0", claimed: "0" })
    }

    function row(fields) {
        const base = JSON.parse(JSON.stringify(Fixtures.load("settled").items[0]))
        for (const k in fields) base[k] = fields[k]
        return base
    }

    function createPage(account) {
        if (account !== undefined) backendStub.activeAccount = account
        page = createTemporaryObject(pageComponent, testCase, {
            accounts: Fixtures.load("accounts"),
            activeAccount: backendStub.activeAccount
        })
        verify(page !== null)
        tryVerify(function() { return findChild(page, "settledList").count === testCase.settledReply.items.length })
        return page
    }

    function ready(account) {
        const p = createPage(account)
        tryCompare(findChild(p, "settledDetail"), "auctionId", auctionReply.id)
        return p
    }

    function callsOf(kind, viewName) {
        return calls.filter(function(c) {
            return c.call === kind && (viewName === undefined || (c.req && c.req.view === viewName))
        })
    }

    function rowNames(list) {
        const names = []
        for (let i = 0; i < list.count; i++) {
            const r = list.itemAtIndex(i)
            names.push(r ? r.objectName : "<none>")
        }
        return names
    }

    // ---- "Where the N went" ----------------------------------------------------------------------

    // The segments are the view's own fields (debt_repaid, penalty, shortfall), never derived; the two that
    // were raised add up to `raised`, and the widths split the track in the same proportions.
    function checkBar(p) {
        const debt = findChild(p, "outcomeDebt")
        const penalty = findChild(p, "outcomePenalty")
        const shortfall = findChild(p, "outcomeShortfall")
        compare(debt.value, auctionReply.debt_repaid)
        compare(penalty.value, auctionReply.penalty)
        compare(shortfall.value, auctionReply.shortfall)
        compare(Format.add(debt.value, penalty.value), auctionReply.raised)
        const track = findChild(p, "outcomeTrack")
        fuzzyCompare(debt.width + penalty.width + shortfall.width, track.width, 1)
        const whole = Format.add(auctionReply.raised, auctionReply.shortfall)
        fuzzyCompare(debt.width, track.width * Format.ratio(auctionReply.debt_repaid, whole), 0.5)
        fuzzyCompare(penalty.width, track.width * Format.ratio(auctionReply.penalty, whole), 0.5)
    }

    function test_outcome_bar_sums_to_raised() {
        targetRaised()
        let p = ready()
        checkBar(p)
        compare(findChild(p, "outcomeHeading").text, "Where the 660,000 stable went")
        compare(findChild(p, "legendDebt").text, "Debt repaid 600,000")
        compare(findChild(p, "legendPenalty").text, "Penalty to the buffer 60,000")
        compare(findChild(p, "legendShortfall").text, "Shortfall 0")

        // Ended short: the shortfall is the debt not repaid, shown next to what was raised, not inside it.
        endedShort()
        p.refresh()
        tryCompare(findChild(p, "outcomeDebt"), "value", "450000")
        checkBar(p)
        compare(findChild(p, "outcomeHeading").text, "Where the 450,000 stable went")
        compare(findChild(p, "legendShortfall").text, "Shortfall 150,000")
        verify(findChild(p, "outcomeNote").text.indexOf("not repaid") >= 0, findChild(p, "outcomeNote").text)
    }

    // The real fixture: nothing raised, the whole 600,000 debt short.
    function test_outcome_bar_with_nothing_raised() {
        const p = ready()
        checkBar(p)
        compare(findChild(p, "outcomeHeading").text, "Nothing was raised")
        fuzzyCompare(findChild(p, "outcomeShortfall").width, findChild(p, "outcomeTrack").width, 1)
        compare(findChild(p, "legendShortfall").text, "Shortfall 600,000")
    }

    // ---- List ------------------------------------------------------------------------------------

    // Outcome text per row: "x to claim" (accent), "n short" (error), "Claimed"; the order is the view's
    // (the scan cursor's id order: the chain records no settle time).
    function test_list_outcomes_in_view_order() {
        settledReply = { items: [
            row({ id: "A1", debt_repaid: "600000", penalty: "60000", raised: "660000", shortfall: "0",
                  sold: "400000", leftover: "75000", claimed: "0" }),
            row({ id: "A2", debt_repaid: "450000", raised: "450000", shortfall: "150000", sold: "475000" }),
            row({ id: "A3", debt_repaid: "600000", penalty: "60000", raised: "660000", shortfall: "0",
                  sold: "400000", leftover: "75000", claimed: "75000" })
        ], scanned_to: 931, skipped: [] }
        const p = createPage()
        const list = findChild(p, "settledList")
        compare(rowNames(list), ["settledItem_A1", "settledItem_A2", "settledItem_A3"])
        compare(findChild(p, "settledScanned").text, "Scanned to block 931")
        const claim = findChild(findChild(p, "settledItem_A1"), "settledItemOutcome")
        compare(claim.text, "75,000 to claim")
        verify(Qt.colorEqual(claim.color, findChild(p, "settledItem_A1").accentColor))
        const shortText = findChild(findChild(p, "settledItem_A2"), "settledItemOutcome")
        compare(shortText.text, "150,000 short")
        verify(Qt.colorEqual(shortText.color, findChild(p, "settledItem_A2").errorColor))
        compare(findChild(findChild(p, "settledItem_A3"), "settledItemOutcome").text, "Claimed")
        compare(findChild(findChild(p, "settledItem_A1"), "settledItemStatus").text, "Target raised")
        compare(findChild(findChild(p, "settledItem_A2"), "settledItemStatus").text, "Sold out")
        compare(findChild(findChild(p, "settledItem_A1"), "settledItemLot").text, "475,000 collateral")
    }

    // ---- Detail ----------------------------------------------------------------------------------

    // KPIs: raised of target, collateral sold of the lot, bids (R35: no end time, so no "ran for").
    function test_kpis_and_facts() {
        targetRaised()
        const p = ready()
        compare(findChild(p, "kpiLabel_0").text, "Raised")
        compare(findChild(p, "kpiValue_0").text, "660,000")
        compare(findChild(p, "kpiCaption_0").text, "of 660,000 stable")
        compare(findChild(p, "kpiValue_1").text, "400,000")
        compare(findChild(p, "kpiCaption_1").text, "of 475,000")
        compare(findChild(p, "kpiLabel_2").text, "Bids")
        compare(findChild(p, "kpiValue_2").text, "3")
        compare(findChild(p, "settledBadge").text, "Target raised")
        compare(findChild(findChild(p, "factLeftover"), "factValue").text, "75,000")
        compare(findChild(findChild(p, "factTarget"), "factValue").text, "660,000")
        compare(findChild(p, "settledOwner").text, "Owner owner2")
    }

    // ---- Claim card states -----------------------------------------------------------------------

    function test_claim_disabled_for_non_owner() {
        targetRaised()
        const p = ready("keeper")
        const button = findChild(p, "claimSubmit")
        verify(button.visible)
        verify(!button.enabled)
        compare(findChild(p, "claimReason").text,
                "Only the owner of position 77ZY…nQjP can claim. Switch account to owner2.")
        button.clicked()
        compare(callsOf("submit").length, 0)
        compare(callsOf("view", "auction").length, 1, "a disabled claim reads nothing")
    }

    // An owner the wallet does not hold is named by its short id.
    function test_claim_reason_names_an_unknown_owner_by_id() {
        targetRaised()
        variant({ owner: "9abcDEFghijkLMNopqrsTUVwxyz123456789ABCDwxyz" })
        const p = ready("owner2")
        verify(!findChild(p, "claimSubmit").enabled)
        compare(findChild(p, "claimReason").text,
                "Only the owner of position 77ZY…nQjP can claim. Its owner 9abc…wxyz is not in this wallet.")
        compare(findChild(p, "settledOwner").text, "Owner 9abc…wxyz")
    }

    function test_claim_enabled_for_the_owner() {
        targetRaised()
        const p = ready("owner2")
        const button = findChild(p, "claimSubmit")
        tryVerify(function() { return button.enabled })
        compare(button.text, "Claim leftover")
        compare(findChild(p, "claimAmount").text, "75,000")
        verify(!findChild(p, "claimReason").visible)
    }

    function test_claimed_shows_amount() {
        targetRaised()
        variant({ claimed: "75000" })
        const p = ready("owner2")
        const claimed = findChild(p, "claimClaimed")
        verify(claimed.visible)
        compare(claimed.text, "Claimed 75,000 collateral")
        verify(!findChild(p, "claimSubmit").visible)
        compare(findChild(findChild(p, "settledItem_" + auctionReply.id), "settledItemOutcome").text, "Claimed")
    }

    function test_shortfall_notice_without_claim() {
        endedShort()
        const p = ready("owner2")
        const notice = findChild(p, "settledShortNotice")
        verify(notice.shown)
        compare(notice.severity, LogosNotice.Warning)
        compare(notice.title, "150,000 stable short")
        compare(notice.message,
                "Raised 450,000 of 660,000. The 150,000 of debt not repaid is now unbacked debt; the buffer covers it with Settle debt on System.")
        verify(findChild(p, "openSystemLink").visible)
        verify(!findChild(p, "claimSubmit").visible)
    }

    function test_nothing_left_over() {
        soldOut()
        const p = ready("owner2")
        verify(!findChild(p, "settledShortNotice").shown)
        verify(!findChild(p, "claimSubmit").enabled)
        compare(findChild(p, "claimNothing").text, "Nothing is left over on this auction, so there is nothing to claim.")
        compare(findChild(p, "settledBadge").text, "Sold out")
    }

    // Ended short: the link switches to the System tab and asks it for Accounting (Main.systemSection).
    function test_shortfall_links_to_system() {
        backendStub.activeAccount = "owner2"
        const component = Qt.createComponent("../../qml/Main.qml")
        compare(component.status, Component.Ready, component.errorString())
        const main = createTemporaryObject(component, testCase, { width: 1100, height: 720 })
        verify(main !== null)
        const pages = findChild(main, "pages")
        mouseClick(findChild(main, "tabSettled"))
        tryCompare(pages, "currentIndex", 2)
        const notice = findChild(main, "settledShortNotice")
        tryCompare(notice, "shown", true)
        compare(notice.title, "600,000 stable short")
        const link = findChild(main, "openSystemLink")
        verify(link.visible)
        link.activate()
        compare(pages.currentIndex, 3)
        verify(findChild(main, "tabSystem").checked)
        compare(main.systemSection, "accounting")
    }

    // ---- Claim -----------------------------------------------------------------------------------

    function openDialog(p) {
        const button = findChild(p, "claimSubmit")
        tryVerify(function() { return button.enabled })
        const reads = callsOf("view", "auction").length
        button.clicked()
        // A fresh read of the auction is the claim's quote.
        compare(callsOf("view", "auction").length, reads + 1)
        const dialog = findChild(p, "settledTxDialog")
        tryCompare(dialog, "opened", true)
        return dialog
    }

    function test_claim_confirms_and_submits() {
        targetRaised()
        const p = ready("owner2")
        const dialog = openDialog(p)
        compare(dialog.request.action, "claim_leftover")
        compare(dialog.request.from, ownerId)
        findChild(dialog, "settledTxConfirm").clicked()
        const submits = callsOf("submit")
        compare(submits.length, 1)
        compare(JSON.stringify(submits[0].req), JSON.stringify({
            action: "claim_leftover",
            owner: "owner2",
            owner_holding: "owner2-coll",
            auction: auctionReply.id
        }))
        const lane = findChild(p, "settledTxLane")
        tryCompare(lane, "state", "waiting")
        verify(!findChild(p, "claimSubmit").visible)
    }

    // The fresh read says it was claimed meanwhile: no dialog, the card shows the claimed state.
    function test_claim_rechecks_the_fresh_read() {
        targetRaised()
        const p = ready("owner2")
        const button = findChild(p, "claimSubmit")
        tryVerify(function() { return button.enabled })
        auctionReply.claimed = "75000"
        button.clicked()
        verify(!findChild(p, "settledTxDialog").opened)
        tryVerify(function() { return findChild(p, "claimClaimed").visible })
    }

    function test_refused_claim_shows_state_notice() {
        targetRaised()
        submitReply = { error: "the engine would reject this: NothingToClaim (code 40)", kind: "refused" }
        const p = ready("owner2")
        findChild(openDialog(p), "settledTxConfirm").clicked()
        const notice = findChild(p, "claimNotice")
        tryCompare(notice, "shown", true)
        compare(notice.severity, LogosNotice.Error)
        compare(notice.message, "Claim: the engine would reject this: NothingToClaim (code 40)")
    }

    function test_tx_names_are_scoped_to_the_settled_page() {
        targetRaised()
        const p = ready("owner2")
        for (const name of ["settledTxDialog", "settledTxConfirm", "settledTxCancel", "settledTxLane", "settledTxRequote"])
            verify(findChild(p, name) !== null, name)
        for (const plain of ["txDialog", "txConfirm", "txLane"])
            compare(findChild(p, plain), null, plain)
    }

    function test_lane_done_refreshes_the_views() {
        targetRaised()
        const p = ready("owner2")
        findChild(openDialog(p), "settledTxConfirm").clicked()
        const before = callsOf("view", "settled").length
        statusReply = { txId: "tx1", state: "included", step: 0, steps: 1 }
        const lane = findChild(p, "settledTxLane")
        lane.poll()
        compare(lane.state, "done")
        tryVerify(function() { return callsOf("view", "settled").length === before + 1 })
    }

    // ---- Failures and rescan (Review Focus 5) ----------------------------------------------------

    function test_settled_failure_is_node_trouble() {
        settledReply = { error: "the node did not answer", kind: "chain", items: [] }
        page = createTemporaryObject(pageComponent, testCase, { accounts: Fixtures.load("accounts") })
        const notice = findChild(page, "settledNotice")
        tryCompare(notice, "shown", true)
        compare(notice.severity, LogosNotice.Warning)
        compare(notice.message, "Settled auctions: the node did not answer")
    }

    function test_rescan_after_restart_clears_list() {
        const p = createPage()
        const list = findChild(p, "settledList")
        compare(list.count, 1)
        rescanQueue = [
            { done: false, head: 5, note: "the chain was replaced", restarted: true, scanned_to: 2 },
            { done: true, head: 5, note: null, restarted: false, scanned_to: 5 }
        ]
        settledReply = { items: [], scanned_to: 5, skipped: [] }
        const before = callsOf("view", "settled").length
        findChild(p, "settledRescan").activate()
        tryVerify(function() { return callsOf("view", "settled").length === before + 1 })
        compare(callsOf("rescan").length, 2)
        compare(listCountAtRescan[1], 0, "the restarted page emptied the list before paging on")
        compare(list.count, 0)
        compare(findChild(p, "settledScanned").text, "Scanned to block 5")
    }
}
