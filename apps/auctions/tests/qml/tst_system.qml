import QtQuick
import QtTest

import Logos.Controls

import "../../qml/system"
import "../../qml/common/Format.js" as Format
import "Fixtures.js" as Fixtures

// The System screen (board v5) against a stub backend that answers with the real fixtures:
//   overview:            buffer 2,000, unbacked 1,000, 500 of it on a live debt auction, freeze threshold 1,000,
//                        a live surplus auction (500 left) and a live debt auction (500 left), host not frozen,
//                        no earlier debt auction end (earliest_debt_start_ms null);
//   quote_settle_debt:   the engine's Accounting::settle_debt, min(amount, buffer, unbacked − on_auction) = 500;
//   quote_take_surplus:  100 stable filled for 40 protocol tokens (at most 41 with 0.5 % slippage);
//   quote_take_debt:     100 stable filled for 65 protocol tokens minted (at least 64).
// States no fixture has (idle auctions, a relist delay, a frozen host) are variants of the real JSON.
TestCase {
    id: testCase
    name: "System"
    width: 1100
    height: 760
    visible: true
    when: windowShown

    readonly property string callerId: "7JbHyBcLCadmDaehnRpwUoFXnLrH5CVMf313TFT2o6oA"
    readonly property string takerId: "8PnJ5BQvgTJ4AshMPka8dqFfErymYLMRdqLve6qB9tkf"

    property var overviewReply: ({})
    property var settleQuote: ({})
    property var surplusQuote: ({})
    property var debtQuote: ({})
    property var submitReply: ({})
    // With deferSubmit the stub holds submit replies back; the test answers them through `deferredSubmits`.
    property bool deferSubmit: false
    property var deferredSubmits: []
    property var statusReply: ({})
    property var calls: []
    property real fakeNow: 1700000000000
    property var page: null

    function initTestCase() {
        Qt.application.organization = "auctions-qml-tests"
        Qt.application.domain = "auctions.test"
    }

    QtObject {
        id: backendStub
        property string network: "localnet"
        property bool devNetwork: true
        property string activeAccount: "caller"
        property bool walletOpen: true
        property bool nodeReachable: true
        property string lastError: ""
        function view(req) { testCase.calls.push({ call: "view", req: req }); return { call: "view", req: req } }
        function accounts() { return { call: "accounts" } }
        function rescan(req) { return { call: "rescan" } }
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
                case "overview": ok(copy(testCase.overviewReply)); return
                case "quote_settle_debt": ok(copy(testCase.settleQuote)); return
                case "quote_take_surplus": ok(copy(testCase.surplusQuote)); return
                case "quote_take_debt": ok(copy(testCase.debtQuote)); return
                case "live": ok(Fixtures.load("live")); return
                case "at_risk": ok(Fixtures.load("at_risk")); return
                case "settled": ok(Fixtures.load("settled")); return
                case "auction": ok(Fixtures.load("auction")); return
                case "position": ok(Fixtures.load("position")); return
                }
                ok({ error: "unknown view", kind: "bad_request" })
                return
            case "rescan": ok(Fixtures.load("scan_done")); return
            case "submit":
                if (testCase.deferSubmit) {
                    testCase.deferredSubmits.push(ok)
                    return
                }
                ok(copy(testCase.submitReply))
                return
            case "txStatus": ok(copy(testCase.statusReply)); return
            }
        }
    }

    Component {
        id: pageComponent
        SystemPage {
            width: 1060
            height: 740
            backend: backendStub
        }
    }

    function init() {
        overviewReply = Fixtures.load("overview")
        settleQuote = Fixtures.load("quote_settle_debt")
        surplusQuote = Fixtures.load("quote_take_surplus")
        debtQuote = Fixtures.load("quote_take_debt")
        submitReply = { txId: "tx1", hash: "h", what: "settle_debt", step: 0, steps: 1, tracking: { signer: callerId } }
        statusReply = { txId: "tx1", state: "pending", blocks_waited: 1, step: 0, steps: 1 }
        calls = []
        deferSubmit = false
        deferredSubmits = []
        fakeNow = 1700000000000
        page = null
        backendStub.activeAccount = "caller"
    }

    function cleanup() {
        page = null
    }

    // Collect this file's many fixture copies before the next file runs (optional per R41).
    function cleanupTestCase() {
        gc()
    }

    function accounting(fields) {
        for (const k in fields) overviewReply.accounting[k] = fields[k]
    }

    function createPage(section, account) {
        if (account !== undefined) backendStub.activeAccount = account
        page = createTemporaryObject(pageComponent, testCase, {
            accounts: Fixtures.load("accounts"),
            activeAccount: backendStub.activeAccount,
            section: section || "",
            clock: function() { return testCase.fakeNow }
        })
        verify(page !== null)
        tryVerify(function() { return page.overview !== null })
        return page
    }

    function callsOf(kind, viewName) {
        return calls.filter(function(c) {
            return c.call === kind && (viewName === undefined || (c.req && c.req.view === viewName))
        })
    }

    function openDialog(p, buttonName) {
        const button = findChild(p, buttonName)
        tryVerify(function() { return button.enabled }, 5000, buttonName + " enabled")
        button.clicked()
        const dialog = findChild(p, "systemTxDialog")
        tryCompare(dialog, "opened", true)
        return dialog
    }

    function confirm(p) {
        findChild(findChild(p, "systemTxDialog"), "systemTxConfirm").clicked()
    }

    function lastSubmit() {
        const s = callsOf("submit")
        verify(s.length > 0, "a submit was sent")
        return s[s.length - 1].req
    }

    // ---- Brief tests -------------------------------------------------------------------------------

    // R38: the card shows quote_settle_debt's amount, the engine's min(amount, buffer, unbacked − on_auction),
    // never a QML min of its own. In the fixture: min(2,000, 1,000 − 500) = 500.
    function test_settle_debt_quote_is_min_of_buffer_and_unbacked() {
        const p = createPage("accounting")
        const amount = findChild(p, "settleDebtAmount")
        tryCompare(amount, "text", "500")
        const q = settleQuote
        const notOnAuction = Format.subtract(q.unbacked_before, q.debt_on_auction)
        const engineMin = Format.cmp(q.buffer_before, notOnAuction) < 0 ? q.buffer_before : notOnAuction
        compare(q.amount, engineMin, "the fixture follows the engine formula")
        compare(amount.text, Format.amount(q.amount, 0))
        compare(findChild(findChild(p, "settleUnbacked"), "settleRowValue").text, "1,000")
        compare(findChild(findChild(p, "settleOnAuction"), "settleRowValue").text, "500")
        compare(findChild(findChild(p, "settleBuffer"), "settleRowValue").text, "2,000")
        compare(findChild(findChild(p, "settleUnbackedAfter"), "settleRowValue").text, "500")
        compare(findChild(findChild(p, "settleBufferAfter"), "settleRowValue").text, "1,500")
        // A different quote is shown as returned, not recomputed from the overview.
        settleQuote.amount = "123"
        findChild(p, "systemRefresh").activate()
        tryCompare(amount, "text", "123")

        settleQuote.amount = "500"
        const dialog = openDialog(p, "settleDebtSubmit")
        compare(dialog.request.action, "settle_debt")
        compare(dialog.request.from, callerId)
        confirm(p)
        compare(JSON.stringify(lastSubmit()), JSON.stringify({ action: "settle_debt", caller: "caller" }))
        tryCompare(findChild(p, "systemTxLane"), "state", "waiting")
    }

    // ADR-039: start is refused until earliest_debt_start_ms (chain clock); the button says why and a local
    // one-shot timer re-enables it when the time passes (no chain poll, R28).
    function test_start_debt_waits_for_relist_delay() {
        accounting({ debt_live: false, debt_lot_left: null, debt_on_auction: "0",
                     earliest_debt_start_ms: fakeNow + 60000 })
        const p = createPage("debt")
        const button = findChild(p, "startDebt")
        const reason = findChild(p, "startDebtReason")
        tryVerify(function() { return reason.visible })
        verify(!button.enabled)
        verify(reason.text.indexOf("Relist delay (ADR-039)") === 0, reason.text)
        verify(reason.text.indexOf("10 minutes") > 0, reason.text)
        const timer = findChild(p, "relistTimer")
        verify(timer.running)
        compare(timer.interval, 60000)
        button.clicked()
        compare(callsOf("submit").length, 0)
        // The delay passes.
        fakeNow += 60000
        timer.triggered()
        tryVerify(function() { return button.enabled })
        verify(!reason.visible)
        verify(!timer.running)
        submitReply.what = "start_debt"
        openDialog(p, "startDebt")
        confirm(p)
        compare(JSON.stringify(lastSubmit()), JSON.stringify({ action: "start_debt", caller: "caller" }))
    }

    // Host frozen: a Frozen badge in the header on every tab. Overview is not polled (R28): every page that
    // reads it hands it to Main.lastOverview, and the header binds to that.
    function test_frozen_badge_in_header() {
        overviewReply.host.frozen = true
        const component = Qt.createComponent("../../qml/Main.qml")
        compare(component.status, Component.Ready, component.errorString())
        const main = createTemporaryObject(component, testCase, { width: 1100, height: 760 })
        verify(main !== null)
        const badge = findChild(main, "headerFrozen")
        tryCompare(badge, "visible", true)
        compare(badge.text, "Frozen")
        for (const tab of ["tabRisk", "tabSettled", "tabSystem", "tabLive"]) {
            mouseClick(findChild(main, tab))
            verify(badge.visible, tab)
        }
        mouseClick(findChild(main, "tabSystem"))
        mouseClick(findChild(main, "systemItem_host"))
        const notice = findChild(main, "hostFrozenNotice")
        tryCompare(notice, "shown", true)
        compare(notice.severity, LogosNotice.Error)
        compare(notice.title, "New debt is disabled")
        // The host unfreezes; the next overview read clears the badge.
        overviewReply.host.frozen = false
        findChild(main, "systemRefresh").activate()
        tryCompare(badge, "visible", false)
        verify(!notice.shown)
    }

    // The taker pays protocol tokens, which the engine burns (programs/auction_engine/src/surplus.rs
    // take_surplus: Token::Burn(protocol_def, taker_protocol, tokens), then the vault pays the stable).
    function test_take_surplus_shows_burn_note() {
        const p = createPage("surplus", "taker")
        const field = findChild(p, "takeSurplusAmount")
        field.text = "500"
        const note = findChild(p, "takeSurplusBurnNote")
        tryCompare(note, "visible", true)
        compare(note.text, "The 40 protocol tokens you pay are burned. You pay at most 41 (0.5 % slippage).")
        const q = callsOf("view", "quote_take_surplus")
        compare(q[q.length - 1].req.amount, "500")
        compare(findChild(p, "takeSurplusPay").text, "40")
        compare(findChild(p, "takeSurplusGet").text, "100")
        compare(findChild(findChild(p, "takeSurplusLotAfter"), "takeRowValue").text, "400")
        submitReply = { txId: "tx2", what: "take_surplus", step: 0, steps: 1, tracking: { signer: takerId } }
        const dialog = openDialog(p, "takeSurplusSubmit")
        compare(dialog.request.action, "take_surplus")
        verify(dialog.request.note.indexOf("burned") >= 0, dialog.request.note)
        confirm(p)
        compare(JSON.stringify(lastSubmit()), JSON.stringify({
            action: "take_surplus", payer: "taker-protocol", receiver: "taker-stable", amount: "500",
            slippage_bps: surplusQuote.slippage_bps
        }))
    }

    // check_solvency (auction_core system.rs, programs.md): Healthy at or below the threshold; above it Freeze
    // only when no debt auction can ever start, and a refusal while one is live. The copy never promises a freeze
    // and never says the host stays live when it is frozen.
    function test_solvency_verdict_follows_the_engine() {
        accounting({ unbacked_debt: "1500" })        // above 1,000, the fixture's debt auction is live
        let p = createPage("accounting")
        compare(findChild(p, "solvencyVerdict").text,
                "Now 1,500: above the threshold, and a debt auction is live: the check is refused.")
        accounting({ debt_live: false, debt_lot_left: null, debt_on_auction: "0" })
        findChild(p, "systemRefresh").activate()
        tryCompare(findChild(p, "solvencyVerdict"), "text",
                   "Now 1,500: above the threshold. The engine may freeze new debt (when no debt auction can ever start).")
        accounting({ unbacked_debt: "1000" })        // equality is Healthy
        overviewReply.host.frozen = true
        findChild(p, "systemRefresh").activate()
        tryCompare(findChild(p, "solvencyVerdict"), "text",
                   "Now 1,000: at or below the threshold. Healthy: no new freeze; the host stays frozen until its freeze authority unfreezes it.")
    }

    // A Confirm sends one submit; until it is answered the actions stay disabled and act() is ignored.
    function test_no_second_action_while_a_submit_is_in_flight() {
        const p = createPage("accounting")
        deferSubmit = true
        openDialog(p, "observeFeed")
        confirm(p)
        compare(callsOf("submit").length, 1)
        verify(p.busy)
        for (const name of ["observeFeed", "checkSolvency", "settleDebtSubmit"])
            verify(!findChild(p, name).enabled, name)
        const reads = callsOf("view").length
        p.act({ title: "x", read: { view: "overview" }, dialog: function() { return {} }, submit: function() { return {} } })
        compare(callsOf("view").length, reads, "act() while submitting reads nothing")
        verify(!findChild(p, "systemTxDialog").opened)
        deferredSubmits.shift()(submitReply)
        tryCompare(findChild(p, "systemTxLane"), "state", "waiting")
        compare(callsOf("submit").length, 1)
        // An error reply also clears the guard.
        statusReply = { txId: "tx1", state: "included", step: 0, steps: 1 }
        findChild(p, "systemTxLane").poll()
        tryVerify(function() { return !p.busy })
        openDialog(p, "checkSolvency")
        confirm(p)
        verify(p.busy)
        deferredSubmits.shift()({ error: "the engine would reject this: RecoveryInProgress (code 52)", kind: "refused" })
        verify(!p.busy)
        tryVerify(function() { return findChild(p, "checkSolvency").enabled })
    }

    // ---- Accounting --------------------------------------------------------------------------------

    function test_accounting_kpis_and_coverage() {
        const p = createPage("accounting")
        compare(findChild(p, "kpiLabel_0").text, "Buffer")
        compare(findChild(p, "kpiValue_0").text, "2,000")
        compare(findChild(p, "kpiValue_1").text, "1,000")
        compare(findChild(p, "kpiValue_2").text, "500")
        compare(findChild(p, "kpiValue_3").text, "1,000")
        compare(findChild(p, "systemBlock").text, "Read at block 3")
        // Two bars on one scale (the largest of buffer, unbacked and threshold) and the threshold tick.
        const track = findChild(p, "coverTrackBuffer")
        fuzzyCompare(findChild(p, "coverBarBuffer").width, track.width, 1)
        fuzzyCompare(findChild(p, "coverBarUnbacked").width, track.width * 0.5, 1)
        fuzzyCompare(findChild(p, "coverTick").x, findChild(p, "coverTicks").width * 0.5, 2)
        compare(findChild(p, "coverScale").text, "0 – 2,000 stable")
        compare(findChild(p, "solvencyVerdict").text, "Now 1,000: at or below the threshold. Healthy: no freeze.")
    }

    function test_upkeep_actions_submit_as_caller() {
        const p = createPage("accounting")
        compare(findChild(p, "observeFeed").variant, LogosButton.Variant.Secondary)
        compare(findChild(p, "observeFeed").height, 32)
        let dialog = openDialog(p, "observeFeed")
        compare(dialog.request.action, "observe_feed")
        confirm(p)
        compare(JSON.stringify(lastSubmit()), JSON.stringify({ action: "observe_feed", caller: "caller" }))
        // The lane is busy: the other actions wait.
        verify(!findChild(p, "checkSolvency").enabled)
        statusReply = { txId: "tx1", state: "included", step: 0, steps: 1 }
        const reads = callsOf("view", "overview").length
        findChild(p, "systemTxLane").poll()
        tryCompare(findChild(p, "systemTxLane"), "state", "done")
        tryVerify(function() { return callsOf("view", "overview").length > reads }, 5000, "lane done re-reads the overview")
        dialog = openDialog(p, "checkSolvency")
        compare(findChild(p, "systemTxLane").txId, "", "a new action clears the finished lane")
        confirm(p)
        compare(JSON.stringify(lastSubmit()), JSON.stringify({ action: "check_solvency", caller: "caller" }))
    }

    function test_nothing_to_settle_disables_the_button() {
        settleQuote = { amount: "0", buffer_before: "0", buffer_after: "0", unbacked_before: "1000",
                        unbacked_after: "1000", debt_on_auction: "500" }
        const p = createPage("accounting")
        tryCompare(findChild(p, "settleDebtAmount"), "text", "0")
        verify(!findChild(p, "settleDebtSubmit").enabled)
        verify(findChild(p, "settleDebtReason").visible)
    }

    // A host or engine refusal (kind "refused") and a wrong network (kind "mismatch", R22) are StateNotices.
    function test_refused_and_mismatch_are_state_notices() {
        const p = createPage("accounting")
        submitReply = { error: "the engine would reject this: RecoveryInProgress (code 52)", kind: "refused" }
        openDialog(p, "checkSolvency")
        confirm(p)
        const notice = findChild(p, "systemActionNotice")
        tryCompare(notice, "shown", true)
        compare(notice.severity, LogosNotice.Error)
        compare(notice.message, "Check solvency: the engine would reject this: RecoveryInProgress (code 52)")
        submitReply = { error: "this deployment is not a dev network", kind: "mismatch" }
        openDialog(p, "observeFeed")
        verify(!notice.shown, "a new action clears the old notice")
        confirm(p)
        tryCompare(notice, "shown", true)
        compare(notice.severity, LogosNotice.Error)
        compare(notice.message, "Observe price feed: this deployment is not a dev network")
    }

    function test_overview_failure_is_node_trouble() {
        overviewReply = { error: "the node did not answer", kind: "chain" }
        page = createTemporaryObject(pageComponent, testCase, { accounts: Fixtures.load("accounts"), section: "" })
        const notice = findChild(page, "systemNotice")
        tryCompare(notice, "shown", true)
        compare(notice.severity, LogosNotice.Warning)
        compare(notice.message, "System: the node did not answer")
    }

    // ---- List and section --------------------------------------------------------------------------

    function test_list_verdicts() {
        const p = createPage("")
        compare(p.selected, "accounting")
        compare(findChild(findChild(p, "systemItem_accounting"), "systemItemVerdict").text, "Unbacked")
        compare(findChild(findChild(p, "systemItem_accounting"), "systemItemCaption").text, "1,000 stable unbacked")
        compare(findChild(findChild(p, "systemItem_surplus"), "systemItemVerdict").text, "Live")
        compare(findChild(findChild(p, "systemItem_surplus"), "systemItemCaption").text, "500 stable left")
        compare(findChild(findChild(p, "systemItem_debt"), "systemItemVerdict").text, "Live")
        compare(findChild(findChild(p, "systemItem_host"), "systemItemVerdict").text, "Live")
        compare(findChild(findChild(p, "systemItem_host"), "systemItemCaption").text, "Freeze threshold 1,000")
        mouseClick(findChild(p, "systemItem_host"))
        compare(p.selected, "host")
        compare(findChild(findChild(p, "hostMinCr"), "factValue").text, "200%")
    }

    function test_section_selects_the_row() {
        const p = createPage("surplus")
        compare(p.selected, "surplus")
        p.select("accounting")
        compare(p.selected, "accounting")
        verify(findChild(p, "systemItem_accounting").highlighted)
    }

    // ---- Surplus and debt --------------------------------------------------------------------------

    function test_start_surplus_disabled_while_live() {
        const p = createPage("surplus")
        verify(!findChild(p, "startSurplus").enabled)
        verify(findChild(p, "startSurplusReason").text.indexOf("A surplus auction is live") === 0)
        accounting({ surplus_live: false, surplus_lot_left: null })
        findChild(p, "systemRefresh").activate()
        const button = findChild(p, "startSurplus")
        tryVerify(function() { return button.enabled })
        verify(!findChild(p, "takeSurplusAmount").visible)
        submitReply.what = "start_surplus"
        openDialog(p, "startSurplus")
        confirm(p)
        compare(JSON.stringify(lastSubmit()), JSON.stringify({ action: "start_surplus", caller: "caller" }))
    }

    function test_take_debt_submits_as_taker() {
        const p = createPage("debt", "taker")
        verify(findChild(p, "startDebtReason").text.indexOf("A debt auction is live") === 0)
        findChild(p, "takeDebtAmount").text = "5000"
        const note = findChild(p, "takeDebtNote")
        tryCompare(note, "visible", true)
        compare(findChild(p, "takeDebtPay").text, "100")
        compare(findChild(p, "takeDebtGet").text, "65")
        verify(note.text.indexOf("at least 64") >= 0, note.text)
        submitReply = { txId: "tx3", what: "take_debt", step: 0, steps: 1, tracking: { signer: takerId } }
        openDialog(p, "takeDebtSubmit")
        confirm(p)
        compare(JSON.stringify(lastSubmit()), JSON.stringify({
            action: "take_debt", payer: "taker-stable", receiver: "taker-protocol", amount: "5000",
            slippage_bps: debtQuote.slippage_bps
        }))
    }

    function test_take_needs_a_whole_amount() {
        const p = createPage("surplus", "taker")
        const field = findChild(p, "takeSurplusAmount")
        field.text = "0"
        wait(500)
        verify(!findChild(p, "takeSurplusSubmit").enabled)
        compare(callsOf("view", "quote_take_surplus").length, 0)
    }

    function test_tx_names_are_scoped_to_the_system_page() {
        const p = createPage("accounting")
        for (const name of ["systemTxDialog", "systemTxConfirm", "systemTxCancel", "systemTxLane", "systemTxRequote"])
            verify(findChild(p, name) !== null, name)
        for (const plain of ["txDialog", "txConfirm", "txLane"])
            compare(findChild(p, plain), null, plain)
    }
}
