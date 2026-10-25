import QtQuick
import QtTest

import Logos.Controls

import "../../qml/risk"
import "Fixtures.js" as Fixtures

// The At-risk positions screen (board v4) against a stub backend that answers with the real fixtures:
// view at_risk / position / overview / quote_liquidation, rescan pages, submit and txStatus.
TestCase {
    id: testCase
    name: "Risk"
    width: 1100
    height: 720
    visible: true
    when: windowShown

    property var atRiskReply: ({})
    property var positionReply: ({})
    property var overviewReply: ({})
    property var quoteReply: ({})
    property var submitReply: ({})
    property var statusReply: ({})
    property var rescanQueue: []
    property var calls: []
    property var listCountAtRescan: []
    property var page: null

    QtObject {
        id: backendStub
        function view(req) { testCase.calls.push({ call: "view", req: req }); return { call: "view", req: req } }
        function rescan(req) {
            const list = testCase.page ? findChild(testCase.page, "riskList") : null
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
            case "view":
                switch (promise.req.view) {
                case "at_risk": ok(copy(testCase.atRiskReply)); return
                case "position": ok(copy(testCase.positionReply)); return
                case "overview": ok(copy(testCase.overviewReply)); return
                case "quote_liquidation": ok(copy(testCase.quoteReply)); return
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
        RiskPage {
            width: 1020
            height: 560
            backend: backendStub
            activeAccount: "keeper"
        }
    }

    function init() {
        atRiskReply = Fixtures.load("at_risk")
        positionReply = Fixtures.load("position")
        overviewReply = Fixtures.load("overview")
        quoteReply = Fixtures.load("quote_liquidation")
        submitReply = { txId: "tx1", hash: "h", what: "liquidate", step: 0, steps: 1,
                        tracking: { signer: "5HSaTMUz6UMtMRwx8yH7Z9dHWLNF9G4op6dh8DM5KKeE" } }
        statusReply = { txId: "tx1", state: "pending", blocks_waited: 1, step: 0, steps: 1 }
        rescanQueue = []
        calls = []
        listCountAtRescan = []
        page = null
    }

    function cleanup() {
        page = null
    }

    function createPage() {
        page = createTemporaryObject(pageComponent, testCase, { accounts: Fixtures.load("accounts") })
        verify(page !== null)
        tryVerify(function() { return findChild(page, "riskList").count === testCase.atRiskReply.items.length })
        return page
    }

    function ready() {
        const p = createPage()
        tryCompare(findChild(p, "positionDetail"), "positionId", positionReply.position)
        return p
    }

    function callsOf(kind, viewName) {
        return calls.filter(function(c) {
            return c.call === kind && (viewName === undefined || (c.req && c.req.view === viewName))
        })
    }

    function row(fields) {
        const base = JSON.parse(JSON.stringify(Fixtures.load("at_risk").items[0]))
        for (const k in fields) base[k] = fields[k]
        return base
    }

    function rowNames(list) {
        const names = []
        for (let i = 0; i < list.count; i++) {
            const r = list.itemAtIndex(i)
            names.push(r ? r.objectName : "<none>")
        }
        return names
    }

    // A healthy position as the `position` view returns it: no quote, the host's refusal.
    function healthy() {
        positionReply.liquidatable = false
        positionReply.cr_bps = 25000
        positionReply.quote = null
        positionReply.refused = "the host would reject this: position is collateralized, not liquidatable (host hook)"
    }

    // ---- List ---------------------------------------------------------------------------------

    function test_sorted_lowest_cr_first() {
        atRiskReply = { items: [
            row({ position: "P15000", cr_bps: 15000, liquidatable: true }),
            row({ position: "P25000", cr_bps: 25000, liquidatable: false }),
            row({ position: "P12000", cr_bps: 12000, liquidatable: true }),
            row({ position: "P16666", cr_bps: 16666, liquidatable: true })
        ], scanned_to: 872, skipped: [] }
        const p = createPage()
        const list = findChild(p, "riskList")
        compare(rowNames(list), ["riskItem_P12000", "riskItem_P15000", "riskItem_P16666", "riskItem_P25000"])
        compare(findChild(p, "riskHeading").text, "Lowest CR first")
        compare(findChild(p, "riskScanned").text, "Scanned to block 872")
        // The first row (lowest CR) is the one selected and read.
        verify(findChild(p, "riskItem_P12000").selected)
        tryVerify(function() { return callsOf("view", "position").length > 0 })
        compare(callsOf("view", "position")[0].req.id, "P12000")
    }

    function test_position_without_cr_sorts_last() {
        atRiskReply = { items: [
            row({ position: "PNULL", cr_bps: null, liquidatable: false }),
            row({ position: "P15000", cr_bps: 15000 }),
            row({ position: "P12000", cr_bps: 12000 })
        ], scanned_to: 3, skipped: [] }
        const p = createPage()
        compare(rowNames(findChild(p, "riskList")), ["riskItem_P12000", "riskItem_P15000", "riskItem_PNULL"])
        compare(findChild(findChild(p, "riskItem_PNULL"), "riskItemCr").text, "CR —")
    }

    // cr_bps is a u64: beyond an int's range the gauge still clamps instead of wrapping.
    function test_huge_cr_clamps_the_gauge() {
        healthy()
        positionReply.cr_bps = 5000000000
        const p = ready()
        compare(findChild(p, "crGauge").markerRatio, 1)
        compare(findChild(p, "crValue").text, "50,000,000.0%")
    }

    function test_row_shows_cr_badge_and_amounts() {
        const p = createPage()
        const r = findChild(p, "riskItem_" + atRiskReply.items[0].position)
        verify(r !== null)
        // R32: CR floors like the CLI (16666 bps = 166.66 % -> 166.6 %).
        compare(findChild(r, "riskItemCr").text, "CR 166.6%")
        compare(findChild(r, "riskItemMin").text, "min 200%")
        compare(findChild(r, "riskItemBadge").text, "Liquidatable")
        compare(findChild(r, "riskItemAmounts").text, "500,000 collateral · 600,000 stable")
    }

    // ---- Detail -------------------------------------------------------------------------------

    function test_gauge_marker_and_threshold_tick() {
        const p = ready()
        const gauge = findChild(p, "crGauge")
        // 100–250 % axis: 166.66 % -> (16666 - 10000) / 15000; the 200 % tick -> 10000 / 15000.
        fuzzyCompare(gauge.markerRatio, 6666 / 15000, 1e-9)
        fuzzyCompare(gauge.tickRatio, 10000 / 15000, 1e-9)
        compare(findChild(p, "crValue").text, "166.6%")
        compare(findChild(gauge, "crTickLabel").text, "200% liquidation threshold")
        // R17: CR comes from the redemption price, never called an oracle CR.
        compare(findChild(p, "crCaption").text, "At the redemption price · min 200%")
    }

    function test_facts_come_from_the_views() {
        const p = ready()
        compare(findChild(findChild(p, "factCollateral"), "factValue").text, "500,000")
        compare(findChild(findChild(p, "factDebt"), "factValue").text, "600,000")
        compare(findChild(findChild(p, "factRedemption"), "factValue").text, "0.5")
        compare(findChild(findChild(p, "factMarket"), "factValue").text, "0.5")
        compare(findChild(findChild(p, "factRange"), "factValue").text, "0.25–1")
        // nominal_debt × redemption_ray: 600,000 × 0.5.
        compare(findChild(findChild(p, "factDebtValue"), "factValue").text, "300,000")
    }

    // Floored to whole units, as every amount is shown: 600,001 × 0.5 = 300,000.5 -> 300,000.
    function test_debt_value_floors() {
        positionReply.nominal_debt = "600001"
        const p = ready()
        compare(findChild(findChild(p, "factDebtValue"), "factValue").text, "300,000")
    }

    function test_debt_value_needs_the_quote() {
        healthy()
        const p = ready()
        compare(findChild(findChild(p, "factDebtValue"), "factValue").text, "—")
    }

    function test_quote_card_rows() {
        const p = ready()
        compare(findChild(p, "liqDebt").value, "600000")
        compare(findChild(findChild(p, "liqDebt"), "liqRowValue").text, "600,000")
        // R33: amount_to_raise − debt_principal (liquidation.rs:79).
        compare(findChild(findChild(p, "liqPenalty"), "liqRowValue").text, "60,000")
        compare(findChild(findChild(p, "liqTarget"), "liqRowValue").text, "660,000")
        compare(findChild(findChild(p, "liqSeized"), "liqRowValue").text, "500,000")
        compare(findChild(findChild(p, "liqLot"), "liqRowValue").text, "499,975")
        const steps = findChild(p, "liquidateSteps")
        verify(steps.text.indexOf("499,975") >= 0, steps.text)
        verify(steps.text.indexOf("660,000") >= 0, steps.text)
    }

    // The reward is the quote's keeper_reward, and the note states the engine's formula
    // (crates/auction_core/src/liquidation.rs: min(tip + chip × debt, reward cap, penalty), floored at the price).
    function test_keeper_reward_from_quote() {
        positionReply.quote.keeper_reward = "1234567"
        const p = ready()
        compare(findChild(findChild(p, "liqReward"), "liqRowValue").text, "1,234,567")
        const note = findChild(p, "liqRewardNote").text
        verify(note.indexOf("min(tip + chip × debt, reward cap, penalty)") >= 0, note)
        verify(note.indexOf("out of the penalty") >= 0, note)
        verify(note.indexOf("debt floor") >= 0, note)
    }

    function test_will_end_short_shown_when_lot_below_target() {
        // Fixture: lot 499,975 at 0.5 collateral per stable is worth 999,950 stable, over the 660,000 target.
        const p = ready()
        verify(!findChild(p, "liqEndShort").visible)
        // 1.5 collateral per stable: the lot is worth 333,316 stable, under the target.
        positionReply.quote.price_ray = "1500000000000000000000000000"
        p.refresh()
        tryVerify(function() { return findChild(p, "liqEndShort").visible })
        verify(findChild(p, "liqEndShort").text.indexOf("<b>Will end short.</b>") === 0)
        verify(findChild(p, "liqEndShort").text.indexOf("starting discount of 2.0%") >= 0)
    }

    // lot × (RAY − start_discount_ray) against target × price_ray: equal is not short, one more unit is.
    function test_end_short_boundary_uses_the_start_discount() {
        positionReply.quote.lot = "500000"
        positionReply.quote.start_discount_ray = "20000000000000000000000000"
        positionReply.quote.price_ray = "1000000000000000000000000000"
        // 500,000 at a 2% discount raises 490,000: exactly the target.
        positionReply.quote.amount_to_raise = "490000"
        const p = ready()
        verify(!findChild(p, "liqEndShort").visible, "raising exactly the target is not short")
        positionReply.quote.amount_to_raise = "490001"
        p.refresh()
        tryVerify(function() { return findChild(p, "liqEndShort").visible })
        // Without the discount the same lot (worth 500,000) would cover 490,001: the discount decides.
        positionReply.quote.start_discount_ray = "0"
        p.refresh()
        tryVerify(function() { return !findChild(p, "liqEndShort").visible })
    }

    function test_quote_header_names_the_overview_block() {
        overviewReply.block = 872
        const p = ready()
        compare(findChild(p, "liqBlock").text, "at block 872")
        verify(findChild(p, "liqBlock").visible)
    }

    // Board v4 capacity: "N left of CAP", N = cap − reserved − target, bars reserved / cap and target / cap.
    function test_cap_bars_from_the_quote() {
        const p = ready()
        const type = findChild(p, "liqTypeCap")
        compare(findChild(type, "capText").text, "3,680,000 left of 5,000,000")
        fuzzyCompare(type.reservedRatio, 660000 / 5000000, 1e-9)
        fuzzyCompare(type.takesRatio, 660000 / 5000000, 1e-9)
        const global = findChild(p, "liqGlobalCap")
        compare(findChild(global, "capText").text, "8,680,000 left of 10,000,000")
        fuzzyCompare(global.reservedRatio, 660000 / 10000000, 1e-9)
    }

    function test_cap_bar_floors_left_after_at_zero() {
        positionReply.quote.type_reserved = "4500000"
        const p = ready()
        const type = findChild(p, "liqTypeCap")
        compare(findChild(type, "capText").text, "0 left of 5,000,000")
        fuzzyCompare(type.takesRatio, 500000 / 5000000, 1e-9)
    }

    // ---- States -------------------------------------------------------------------------------

    function test_healthy_position_cannot_be_liquidated() {
        atRiskReply.items[0].liquidatable = false
        atRiskReply.items[0].cr_bps = 25000
        healthy()
        const p = ready()
        const button = findChild(p, "liquidateSubmit")
        verify(!button.enabled)
        compare(button.text, "Liquidate & start auction")
        compare(findChild(p, "liquidateReason").text, "CR 250.0% is above the 200% minimum.")
        // A healthy position is an answer, not node trouble: no warning notice.
        verify(!findChild(p, "liquidateNotice").shown)
        verify(!findChild(p, "riskFeedNotice").shown)
        compare(findChild(findChild(p, "riskItem_" + positionReply.position), "riskItemBadge").text, "Healthy")
        button.clicked()
        compare(callsOf("view", "quote_liquidation").length, 0)
    }

    function test_refused_position_shows_state_notice_not_node_trouble() {
        positionReply = Fixtures.load("position_refused")
        atRiskReply.items[0].position = positionReply.position
        const p = ready()
        const notice = findChild(p, "liquidateNotice")
        tryCompare(notice, "shown", true)
        compare(notice.severity, LogosNotice.Error)
        compare(notice.message, "Liquidation: the engine would reject this: ZeroDebt (code 31)")
        verify(!findChild(p, "liquidateSubmit").enabled)
    }

    function test_out_of_range_feed_pauses() {
        overviewReply.oracle.price_ray = "2000000000000000000000000000"
        positionReply.quote = null
        positionReply.refused = "the engine would reject this: PriceFeedOutOfRange (code 23)"
        const p = ready()
        const button = findChild(p, "liquidateSubmit")
        tryCompare(button, "text", "Liquidations paused")
        verify(!button.enabled)
        const notice = findChild(p, "riskFeedNotice")
        verify(notice.shown)
        compare(notice.severity, LogosNotice.Error)
        compare(notice.title, "Price feed out of range")
        compare(notice.message,
                "Market 2 is outside 0.25–1 collateral per stable. Liquidations and bids stay paused until it is back in range.")
        // The engine's refusal is the pause itself, so it is not shown a second time.
        verify(!findChild(p, "liquidateNotice").shown)
    }

    function test_stale_oracle_pauses() {
        overviewReply.oracle.fresh = false
        overviewReply.oracle.age_ms = 4320000
        positionReply.quote = null
        positionReply.refused = "the engine would reject this: PriceFeedStale (code 21)"
        const p = ready()
        const button = findChild(p, "liquidateSubmit")
        tryCompare(button, "text", "Liquidations paused")
        verify(!button.enabled)
        const notice = findChild(p, "riskFeedNotice")
        verify(notice.shown)
        compare(notice.title, "Collateral paused · 12m")
        compare(notice.message,
                "The price feed is 72m old (max 60m). Liquidations and bids are paused until it updates.")
    }

    // The overview can say stale while the position read still carried a quote (the feed aged in between):
    // the pause wins.
    function test_stale_oracle_with_a_quote_still_pauses() {
        overviewReply.oracle.fresh = false
        overviewReply.oracle.age_ms = 4320000
        const p = ready()
        const button = findChild(p, "liquidateSubmit")
        tryCompare(button, "text", "Liquidations paused")
        verify(!button.enabled)
        button.clicked()
        compare(callsOf("view", "quote_liquidation").length, 0)
    }

    function test_admin_paused_collateral_pauses() {
        positionReply.quote = null
        positionReply.refused = "the engine would reject this: Paused (code 20)"
        const p = ready()
        const button = findChild(p, "liquidateSubmit")
        tryCompare(button, "text", "Liquidations paused")
        verify(!button.enabled)
        const notice = findChild(p, "riskFeedNotice")
        verify(notice.shown)
        compare(notice.title, "Collateral paused")
    }

    function test_oracle_failure_disables_liquidate() {
        overviewReply = { error: "the node did not answer", kind: "chain" }
        const p = ready()
        const button = findChild(p, "liquidateSubmit")
        tryCompare(button, "text", "Oracle unknown")
        verify(!button.enabled)
    }

    function test_at_risk_failure_is_node_trouble() {
        atRiskReply = { error: "the node did not answer", kind: "chain", items: [] }
        page = createTemporaryObject(pageComponent, testCase, { accounts: Fixtures.load("accounts") })
        const notice = findChild(page, "riskNotice")
        tryCompare(notice, "shown", true)
        compare(notice.severity, LogosNotice.Warning)
        compare(notice.message, "At-risk positions: the node did not answer")
    }

    // ---- Liquidate ----------------------------------------------------------------------------

    function test_liquidate_confirms_and_submits() {
        const p = ready()
        const button = findChild(p, "liquidateSubmit")
        tryVerify(function() { return button.enabled })
        button.clicked()
        compare(callsOf("view", "quote_liquidation").length, 1)
        compare(callsOf("view", "quote_liquidation")[0].req.position, positionReply.position)
        const dialog = findChild(p, "riskTxDialog")
        tryCompare(dialog, "opened", true)
        compare(dialog.request.action, "liquidate")
        findChild(dialog, "riskTxConfirm").clicked()
        const submits = callsOf("submit")
        compare(submits.length, 1)
        compare(JSON.stringify(submits[0].req), JSON.stringify({
            action: "liquidate",
            position: positionReply.position,
            keeper: "keeper",
            keeper_holding: "keeper-coll"
        }))
        const lane = findChild(p, "riskTxLane")
        tryCompare(lane, "state", "waiting")
        verify(!button.visible)
    }

    function test_tx_names_are_scoped_to_the_risk_page() {
        const p = ready()
        for (const name of ["riskTxDialog", "riskTxConfirm", "riskTxCancel", "riskTxLane", "riskTxRequote"])
            verify(findChild(p, name) !== null, name)
        for (const plain of ["txDialog", "txConfirm", "txCancel", "txLane", "txRequote"])
            compare(findChild(p, plain), null, plain)
    }

    function test_refused_submit_shows_notice_not_node_trouble() {
        submitReply = { error: "refusing before signing: the host would reject this liquidation (x)", kind: "refused" }
        const p = ready()
        const button = findChild(p, "liquidateSubmit")
        tryVerify(function() { return button.enabled })
        button.clicked()
        const dialog = findChild(p, "riskTxDialog")
        tryCompare(dialog, "opened", true)
        findChild(dialog, "riskTxConfirm").clicked()
        const notice = findChild(p, "liquidateNotice")
        tryCompare(notice, "shown", true)
        compare(notice.severity, LogosNotice.Error)
    }

    function test_lane_done_refreshes_the_views() {
        const p = ready()
        const button = findChild(p, "liquidateSubmit")
        tryVerify(function() { return button.enabled })
        button.clicked()
        const dialog = findChild(p, "riskTxDialog")
        tryCompare(dialog, "opened", true)
        findChild(dialog, "riskTxConfirm").clicked()
        const before = callsOf("view", "at_risk").length
        const overviews = callsOf("view", "overview").length
        statusReply = { txId: "tx1", state: "included", step: 0, steps: 1 }
        const lane = findChild(p, "riskTxLane")
        lane.poll()
        compare(lane.state, "done")
        tryVerify(function() { return callsOf("view", "at_risk").length === before + 1 })
        verify(callsOf("view", "overview").length > overviews)
    }

    // ---- Rescan (Review Focus 5) ----------------------------------------------------------------

    function test_rescan_after_restart_clears_list() {
        const p = createPage()
        const list = findChild(p, "riskList")
        compare(list.count, 1)
        rescanQueue = [
            { done: false, head: 5, note: "the chain was replaced", restarted: true, scanned_to: 2 },
            { done: true, head: 5, note: null, restarted: false, scanned_to: 5 }
        ]
        atRiskReply = { items: [], scanned_to: 5, skipped: [] }
        const before = callsOf("view", "at_risk").length
        findChild(p, "riskRescan").activate()
        tryVerify(function() { return callsOf("view", "at_risk").length === before + 1 })
        compare(callsOf("rescan").length, 2)
        compare(listCountAtRescan[1], 0, "the restarted page emptied the list before paging on")
        compare(list.count, 0)
        compare(findChild(p, "riskScanned").text, "Scanned to block 5")
    }
}
