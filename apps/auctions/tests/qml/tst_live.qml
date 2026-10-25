import QtQuick
import QtTest

import Logos.Controls

import "../../qml/live"
import "Fixtures.js" as Fixtures

// The Live auctions screen (board v3) against a stub backend that answers with the real fixtures:
// view live / auction / overview / quote_bid, rescan pages, submit and txStatus.
TestCase {
    id: testCase
    name: "Live"
    width: 1100
    height: 720
    visible: true
    when: windowShown

    property var liveReply: ({})
    property var auctionReply: ({})
    property var overviewReply: ({})
    property var quoteReply: ({})
    property var submitReply: ({})
    property var statusReply: ({})
    // Rescan pages handed out oldest first.
    property var rescanQueue: []
    // Every call the stub saw: {call, req}.
    property var calls: []
    // The list's row count at each rescan page request (what the screen showed between pages).
    property var listCountAtRescan: []
    // With deferRescan the stub holds rescan replies; the test answers them through `deferred`.
    property bool deferRescan: false
    property var deferred: []
    // With deferLive the stub holds `view live` replies; the test answers them through `deferredLive`.
    property bool deferLive: false
    property var deferredLive: []
    property var page: null

    function initTestCase() {
        Qt.application.organization = "auctions-qml-tests"
        Qt.application.domain = "auctions.test"
    }

    QtObject {
        id: backendStub
        function view(req) { testCase.calls.push({ call: "view", req: req }); return { call: "view", req: req } }
        function rescan(req) {
            const list = testCase.page ? findChild(testCase.page, "liveList") : null
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
            switch (promise.call) {
            case "view":
                switch (promise.req.view) {
                case "live":
                    if (testCase.deferLive) { testCase.deferredLive.push(ok); return }
                    ok(JSON.parse(JSON.stringify(testCase.liveReply))); return
                case "auction": ok(JSON.parse(JSON.stringify(testCase.auctionReply))); return
                case "overview": ok(JSON.parse(JSON.stringify(testCase.overviewReply))); return
                case "quote_bid": ok(JSON.parse(JSON.stringify(testCase.quoteReply))); return
                }
                ok({ error: "unknown view", kind: "bad_request" })
                return
            case "rescan":
                if (testCase.deferRescan) { testCase.deferred.push(ok); return }
                ok(testCase.rescanQueue.length > 0 ? testCase.rescanQueue.shift() : Fixtures.load("scan_done"))
                return
            case "submit": ok(testCase.submitReply); return
            case "txStatus": ok(testCase.statusReply); return
            }
        }
    }

    Component {
        id: pageComponent
        LivePage {
            width: 1020
            height: 560
            backend: backendStub
            activeAccount: "owner"
        }
    }

    function init() {
        liveReply = Fixtures.load("live")
        auctionReply = Fixtures.load("auction")
        overviewReply = Fixtures.load("overview")
        quoteReply = Fixtures.load("quote_bid")
        submitReply = { txId: "tx1", hash: "h", what: "bid", step: 0, steps: 1,
                        tracking: { signer: "7wx93ACHrg2tR8vHRBcKnVkRJTrtT7zcP3UsxcViopKg" } }
        statusReply = { txId: "tx1", state: "pending", blocks_waited: 1, step: 0, steps: 1 }
        rescanQueue = []
        calls = []
        listCountAtRescan = []
        deferRescan = false
        deferred = []
        deferLive = false
        deferredLive = []
        page = null
    }

    function cleanup() {
        page = null
    }

    function createPage() {
        page = createTemporaryObject(pageComponent, testCase, { accounts: Fixtures.load("accounts") })
        verify(page !== null)
        tryVerify(function() { return findChild(page, "liveList").count === testCase.liveReply.items.length })
        return page
    }

    function callsOf(kind, viewName) {
        return calls.filter(function(c) {
            return c.call === kind && (viewName === undefined || (c.req && c.req.view === viewName))
        })
    }

    function item(fields) {
        const base = JSON.parse(JSON.stringify(Fixtures.load("live").items[0]))
        for (const k in fields) base[k] = fields[k]
        return base
    }

    function rowNames(list) {
        const names = []
        for (let i = 0; i < list.count; i++) {
            const row = list.itemAtIndex(i)
            names.push(row ? row.objectName : "<none>")
        }
        return names
    }

    // ---- List ---------------------------------------------------------------------------------

    // R30: no auction end time exists; the deepest discount comes first, ties: the earlier max_at first.
    function test_list_sorted_deepest_discount_first() {
        liveReply = { items: [
            item({ id: "A111", discount_ray: "37777777777777777777777777", max_at_ms: 1700002100000 }),
            item({ id: "B222", discount_ray: "90000000000000000000000000", max_at_ms: 1700009000000 }),
            item({ id: "C333", discount_ray: "90000000000000000000000000", max_at_ms: 1700001000000 }),
            item({ id: "D444", discount_ray: "100000000000000000000000000", held_at_max: true }),
            item({ id: "E555", discount_ray: "9000000000000000000000000", max_at_ms: 1700000000000 })
        ], scanned_to: 3, skipped: [] }
        const p = createPage()
        const list = findChild(p, "liveList")
        compare(rowNames(list), ["liveItem_D444", "liveItem_C333", "liveItem_B222", "liveItem_A111", "liveItem_E555"])
        compare(findChild(p, "liveHeading").text, "Deepest discount first")
        compare(findChild(p, "liveCount").text, "5 live")
    }

    function test_row_shows_discount_lot_and_raised() {
        const p = createPage()
        const id = liveReply.items[0].id
        const row = findChild(p, "liveItem_" + id)
        verify(row !== null)
        compare(findChild(row, "liveItemDiscount").text, "3.7%")
        compare(findChild(row, "liveItemLot").text, "475,000")
        compare(findChild(row, "liveItemRaised").text, "0% raised")
    }

    // ---- Detail -------------------------------------------------------------------------------

    function test_kpis_show_discount_rate_and_max_time() {
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        compare(findChild(p, "kpiValue_0").text, "3.7%")
        compare(findChild(p, "kpiCaption_0").text, "+0.17% per min")
        compare(findChild(p, "kpiLabel_1").text, "Max discount at")
        compare(findChild(p, "kpiValue_1").text, Qt.formatTime(new Date(auctionReply.max_at_ms), "hh:mm"))
        compare(findChild(p, "kpiValue_2").text, "0")
        compare(findChild(p, "kpiValue_3").text, "475,000")
    }

    function test_held_at_max_kpi() {
        auctionReply.held_at_max = true
        auctionReply.discount_ray = "100000000000000000000000000"
        auctionReply.rate_per_min_ray = "0"
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        compare(findChild(p, "kpiValue_1").text, "Held at max")
    }

    // The "Now" pin sits where the chart maps the auction's discount_ray, at t_eff on the x axis.
    function test_chart_now_pin_matches_discount() {
        const p = createPage()
        const chart = findChild(p, "discountChart")
        tryVerify(function() { return chart.points.length > 0 })
        const pin = findChild(chart, "chartNowPin")
        verify(pin.visible)
        // 37777777777777777777777777 / 1e25 = 3.7777…%
        const expectedY = chart.plotTop + chart.plotHeight * (1 - 3.7777777777 / chart.yMaxPct)
        fuzzyCompare(pin.y + pin.height / 2, expectedY, 0.5)
        const expectedX = chart.plotLeft + chart.plotWidth * auctionReply.t_eff_ms / chart.xMaxMs
        fuzzyCompare(pin.x + pin.width / 2, expectedX, 0.5)
        compare(findChild(chart, "chartNowLabel").text, "Now 3.7%")
        // R30: x runs to max_at + 25 %; the knee names the clock time of the maximum; no expiry marker.
        compare(chart.xMaxMs, (auctionReply.max_at_ms - auctionReply.started_at_ms) * 1.25)
        compare(findChild(chart, "chartKneeLabel").text,
                "Max at " + Qt.formatTime(new Date(auctionReply.max_at_ms), "hh:mm"))
        compare(chart.yMaxPct, 10)
    }

    // ---- Bid card -----------------------------------------------------------------------------

    function test_max_to_finish_fills_left_to_raise() {
        auctionReply.raised = "100000"
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        const max = findChild(p, "bidMax")
        compare(max.text, "Max to finish 560,000")
        max.activate()
        compare(findChild(p, "bidAmount").text, "560000")
        verify(!findChild(p, "bidOverMax").visible)
    }

    function test_over_the_max_shows_use_n() {
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        findChild(p, "bidAmount").text = "700000"
        const over = findChild(p, "bidOverMax")
        tryVerify(function() { return over.visible })
        compare(over.text, "Only 660,000 stable is left to raise.")
        const use = findChild(p, "bidUseMax")
        compare(use.text, "Use 660,000")
        verify(!findChild(p, "bidSubmit").enabled)
        use.activate()
        compare(findChild(p, "bidAmount").text, "660000")
        verify(!over.visible)
        verify(findChild(p, "bidSubmit").enabled)
    }

    function test_you_get_shows_quote_and_slippage_floor() {
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        findChild(p, "bidAmount").text = "100000"
        tryCompare(findChild(p, "bidYouGet"), "text", "≈ 51,963", 3000)
        compare(findChild(p, "bidMinOut").text, "At least 51,703 with 0.5% slippage")
        const quotes = callsOf("view", "quote_bid")
        compare(quotes[quotes.length - 1].req.coin_in_max, "100000")
        compare(quotes[quotes.length - 1].req.slippage_bps, 50)
        // Edit opens the slippage field; a new value re-quotes with it.
        findChild(p, "bidSlippageEdit").activate()
        const slip = findChild(p, "bidSlippage")
        verify(slip.visible)
        slip.text = "1"
        tryVerify(function() {
            const q = callsOf("view", "quote_bid")
            return q[q.length - 1].req.slippage_bps === 100
        }, 3000)
    }

    function test_private_bid_is_disabled() {
        const p = createPage()
        const sw = findChild(p, "bidPrivate")
        verify(!sw.enabled)
        verify(!sw.checked)
        compare(findChild(p, "bidPrivateNote").text, "Public transactions only")
    }

    function test_stale_oracle_disables_bid() {
        overviewReply.oracle.fresh = false
        overviewReply.oracle.age_ms = 3840000
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        findChild(p, "bidAmount").text = "100000"
        const submit = findChild(p, "bidSubmit")
        tryCompare(submit, "text", "Bidding paused")
        verify(!submit.enabled)
        compare(findChild(p, "bidOracleBadge").text, "Stale")
        verify(findChild(p, "bidPausedNotice").visible)
        compare(findChild(p, "bidOraclePrice").text, "1 stable = 0.5 collateral")
    }

    function test_paused_auction_disables_bid() {
        auctionReply.paused = "Paused"
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        findChild(p, "bidAmount").text = "100000"
        const submit = findChild(p, "bidSubmit")
        compare(submit.text, "Bidding paused")
        verify(!submit.enabled)
        verify(findChild(p, "bidPausedNotice").visible)
    }

    // The final bid: quote → dialog (its facts name bid_with_settlement) → submit → lane.
    function test_final_bid_confirms_bid_with_settlement_and_submits() {
        quoteReply.coin_in_max = "230000"
        quoteReply.charged = "230000"
        quoteReply.ends_auction = true
        quoteReply.instruction = "bid_with_settlement"
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        findChild(p, "bidAmount").text = "230000"
        const submit = findChild(p, "bidSubmit")
        tryVerify(function() { return submit.enabled })
        submit.clicked()
        const dialog = findChild(p, "txDialog")
        tryCompare(dialog, "opened", true)
        const facts = dialog.request.facts
        verify(facts.some(function(f) { return f.value === "bid_with_settlement" }),
               JSON.stringify(facts))
        compare(findChild(dialog.contentItem, "quoteRowValue_0").text, "bid_with_settlement")
        compare(dialog.request.action, "bid")
        verify(facts.some(function(f) { return f.label === "Discount" && f.value === "3.7777%" }),
               JSON.stringify(facts))

        // Editing the field behind the open dialog does not change what was confirmed.
        findChild(p, "bidAmount").text = "5"
        findChild(dialog, "txConfirm").clicked()
        const submits = callsOf("submit")
        compare(submits.length, 1)
        const req = submits[0].req
        compare(req.action, "bid")
        compare(req.auction, auctionReply.id)
        compare(req.coin_in_max, "230000")
        compare(req.bidder_stable, "owner-stable")
        compare(req.bidder_collateral, "owner-coll")
        compare(req.slippage_bps, 50)
        compare(req.min_out, quoteReply.min_out)
        const lane = findChild(p, "txLane")
        tryCompare(lane, "visible", true)
        compare(lane.txId, "tx1")
        compare(lane.signer, "owner")
    }

    function test_refused_submit_shows_notice_not_node_trouble() {
        submitReply = { error: "the host refused: auction is settled", kind: "refused" }
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        findChild(p, "bidAmount").text = "100000"
        const submit = findChild(p, "bidSubmit")
        tryVerify(function() { return submit.enabled })
        submit.clicked()
        const dialog = findChild(p, "txDialog")
        tryCompare(dialog, "opened", true)
        findChild(dialog, "txConfirm").clicked()
        const notice = findChild(p, "bidNotice")
        tryCompare(notice, "shown", true)
        compare(notice.severity, LogosNotice.Error)
        compare(notice.message, "Bid: the host refused: auction is settled")
        verify(!findChild(p, "txLane").visible)
    }

    function test_quote_failure_shows_notice_and_opens_no_dialog() {
        quoteReply = { error: "the node did not answer", kind: "chain" }
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        findChild(p, "bidAmount").text = "100000"
        findChild(p, "bidSubmit").clicked()
        const notice = findChild(p, "bidNotice")
        tryCompare(notice, "shown", true)
        compare(notice.severity, LogosNotice.Warning)
        verify(!findChild(p, "txDialog").opened)
    }

    // R28: no overview poll; the screen reads its views again once a lane reaches done.
    function test_lane_done_refreshes_the_views() {
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        findChild(p, "bidAmount").text = "100000"
        const submit = findChild(p, "bidSubmit")
        tryVerify(function() { return submit.enabled })
        submit.clicked()
        const dialog = findChild(p, "txDialog")
        tryCompare(dialog, "opened", true)
        findChild(dialog, "txConfirm").clicked()
        const before = callsOf("view", "live").length
        const overviewsBefore = callsOf("view", "overview").length
        statusReply = { txId: "tx1", state: "included", step: 0, steps: 1 }
        const lane = findChild(p, "txLane")
        lane.poll()
        compare(lane.state, "done")
        tryVerify(function() { return callsOf("view", "live").length === before + 1 })
        verify(callsOf("view", "overview").length > overviewsBefore)
    }

    // The second bid of the story must not read the first bid's "done" as its own.
    function test_a_new_bid_clears_a_finished_lane() {
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        findChild(p, "bidAmount").text = "100000"
        const submit = findChild(p, "bidSubmit")
        tryVerify(function() { return submit.enabled })
        submit.clicked()
        const dialog = findChild(p, "txDialog")
        tryCompare(dialog, "opened", true)
        findChild(dialog, "txConfirm").clicked()
        statusReply = { txId: "tx1", state: "included", step: 0, steps: 1 }
        const lane = findChild(p, "txLane")
        lane.poll()
        compare(lane.state, "done")
        tryCompare(dialog, "opened", false)

        findChild(p, "bidAmount").text = "230000"
        tryVerify(function() { return submit.enabled })
        submit.clicked()
        tryCompare(dialog, "opened", true)
        verify(!lane.visible, "the finished lane is gone while the next bid is confirmed")
        compare(lane.state, "")
    }

    // ---- Fix round 1 ------------------------------------------------------------------------------

    // R31: the engine's minimum charge, which a final bid does not need.
    function test_min_bid_hint() {
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        compare(findChild(p, "bidMin").text, "Min 25 (not for the final bid)")
    }

    // The SDK names the stable holding <label>-stable; a row without one cannot pay.
    function test_account_without_stable_holding_cannot_bid() {
        page = createTemporaryObject(pageComponent, testCase,
                                     { accounts: Fixtures.load("accounts"), activeAccount: "keeper" })
        tryCompare(findChild(page, "auctionDetail"), "auctionId", auctionReply.id)
        findChild(page, "bidAmount").text = "100000"
        const submit = findChild(page, "bidSubmit")
        tryCompare(submit, "text", "No stable holding")
        verify(!submit.enabled)
        const hint = findChild(page, "bidHoldingHint")
        verify(hint.visible)
        compare(hint.text, "keeper has no keeper-stable holding to pay from. Pick an account that holds stable.")
    }

    function test_oracle_failure_disables_bid() {
        overviewReply = { error: "the node did not answer", kind: "chain" }
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        findChild(p, "bidAmount").text = "100000"
        const submit = findChild(p, "bidSubmit")
        tryCompare(submit, "text", "Oracle unknown")
        verify(!submit.enabled)
        const badge = findChild(p, "bidOracleBadge")
        verify(badge.visible)
        compare(badge.text, "Unknown")
        compare(findChild(p, "bidOraclePrice").text, "Oracle unknown")
        verify(findChild(p, "bidPausedNotice").visible)
    }

    function test_invalid_slippage_is_an_error_not_the_old_value() {
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        findChild(p, "bidAmount").text = "100000"
        const submit = findChild(p, "bidSubmit")
        tryVerify(function() { return submit.enabled })
        findChild(p, "bidSlippageEdit").activate()
        const slip = findChild(p, "bidSlippage")
        compare(slip.text, "0.5", "the field starts at the default slippage")
        slip.text = ""
        verify(findChild(p, "bidSlippageError").visible)
        verify(!submit.enabled)
        slip.text = "1."
        verify(findChild(p, "bidSlippageError").visible)
        slip.text = "2"
        verify(!findChild(p, "bidSlippageError").visible)
        verify(submit.enabled)
    }

    // While the lane waits the You-get card collapses; once it is done, Max to finish and Private bid are back
    // (the story's second bid uses Max to finish).
    function test_lane_waiting_collapses_and_done_restores_the_card() {
        const p = createPage()
        tryCompare(findChild(p, "auctionDetail"), "auctionId", auctionReply.id)
        findChild(p, "bidAmount").text = "100000"
        const submit = findChild(p, "bidSubmit")
        tryVerify(function() { return submit.enabled })
        submit.clicked()
        const dialog = findChild(p, "txDialog")
        tryCompare(dialog, "opened", true)
        findChild(dialog, "txConfirm").clicked()
        const lane = findChild(p, "txLane")
        tryCompare(lane, "state", "waiting")
        verify(!findChild(p, "bidYouGet").visible)
        verify(!findChild(p, "bidMax").visible)
        verify(!findChild(p, "bidPrivate").visible)
        statusReply = { txId: "tx1", state: "included", step: 0, steps: 1 }
        lane.poll()
        compare(lane.state, "done")
        verify(lane.visible)
        verify(findChild(p, "bidYouGet").visible)
        verify(findChild(p, "bidMax").visible)
        verify(findChild(p, "bidPrivate").visible)
    }

    // A refresh started before a `restarted` page must not bring the replaced chain's auctions back.
    function test_refresh_racing_a_restarted_page_keeps_the_list_empty() {
        const p = createPage()
        const list = findChild(p, "liveList")
        compare(list.count, 1)
        deferLive = true
        p.refresh()
        compare(deferredLive.length, 1, "a live read is on its way")
        deferRescan = true
        findChild(p, "liveRescan").activate()
        deferred.shift()({ done: false, head: 5, note: "the chain was replaced", restarted: true, scanned_to: 2 })
        compare(list.count, 0)
        // The stale reply (the old chain's auction) lands after the restart, before the rescan is done.
        deferredLive.shift()(Fixtures.load("live"))
        compare(list.count, 0, "the stale reply is dropped")
        deferred.shift()(Fixtures.load("scan_done"))
        // The refresh the finished rescan started answers with the new chain's (empty) list.
        compare(deferredLive.length, 1)
        deferredLive.shift()({ items: [], scanned_to: 5, skipped: [] })
        compare(list.count, 0)
    }

    function test_no_refresh_while_a_rescan_pages() {
        const p = createPage()
        deferRescan = true
        findChild(p, "liveRescan").activate()
        verify(p.rescanning)
        const lives = callsOf("view", "live").length
        p.refresh()
        compare(callsOf("view", "live").length, lives, "a refresh during the rescan sends nothing")
        deferred.shift()(Fixtures.load("scan_done"))
        tryVerify(function() { return callsOf("view", "live").length === lives + 1 })
    }

    // ---- Rescan (Review Focus 5) ----------------------------------------------------------------

    // The localnet was restarted: the page that says so empties the list at once; paging continues to done
    // and the screen reads live again.
    function test_rescan_after_restart_clears_list() {
        const p = createPage()
        const list = findChild(p, "liveList")
        compare(list.count, 1)
        rescanQueue = [
            { done: false, head: 5, note: "the chain was replaced", restarted: true, scanned_to: 2 },
            { done: false, head: 5, note: null, restarted: false, scanned_to: 4 },
            { done: true, head: 5, note: null, restarted: false, scanned_to: 5 }
        ]
        liveReply = { items: [], scanned_to: 5, skipped: [] }
        const livesBefore = callsOf("view", "live").length
        findChild(p, "liveRescan").activate()
        tryVerify(function() { return callsOf("view", "live").length === livesBefore + 1 })
        compare(callsOf("rescan").length, 3, "one request per page, none after done")
        compare(listCountAtRescan[0], 1, "before the first page the list is as it was")
        compare(listCountAtRescan[1], 0, "the restarted page emptied the list before paging on")
        compare(list.count, 0)
    }

    function test_rescan_pages_until_done_one_at_a_time() {
        const p = createPage()
        deferRescan = true
        const link = findChild(p, "liveRescan")
        link.activate()
        link.activate()
        compare(callsOf("rescan").length, 1, "a second click while paging sends nothing")
        verify(p.rescanning)
        deferred.shift()(Fixtures.load("scan"))
        compare(callsOf("rescan").length, 2)
        link.activate()
        compare(callsOf("rescan").length, 2)
        const livesBefore = callsOf("view", "live").length
        deferred.shift()(Fixtures.load("scan_done"))
        verify(!p.rescanning)
        compare(callsOf("rescan").length, 2)
        tryVerify(function() { return callsOf("view", "live").length === livesBefore + 1 })
    }

    function test_rescan_error_stops_and_shows_notice() {
        const p = createPage()
        rescanQueue = [{ error: "the node did not answer", kind: "chain" }]
        findChild(p, "liveRescan").activate()
        verify(!p.rescanning)
        const notice = findChild(p, "liveNotice")
        tryCompare(notice, "shown", true)
        compare(notice.message, "Rescan: the node did not answer")
        compare(callsOf("rescan").length, 1)
    }

    function test_live_view_failure_is_a_notice() {
        liveReply = { error: "the node did not answer", kind: "chain", items: [] }
        page = createTemporaryObject(pageComponent, testCase, { accounts: Fixtures.load("accounts") })
        const notice = findChild(page, "liveNotice")
        tryCompare(notice, "shown", true)
        compare(notice.message, "Live auctions: the node did not answer")
        compare(findChild(page, "liveList").count, 0)
    }
}
