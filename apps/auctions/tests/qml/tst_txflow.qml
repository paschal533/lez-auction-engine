import QtQuick
import QtTest

import Logos.Controls

import "../../qml/shell"
import "Fixtures.js" as Fixtures

// The confirmation dialog and the transaction lane, against a stub backend whose txStatus replies the test
// queues (the shapes auction_engine txStatus returns).
TestCase {
    id: testCase
    name: "TxFlow"
    width: 1100
    height: 720
    visible: true
    when: windowShown

    // Replies txStatus hands out, oldest first; an empty queue answers "pending".
    property var statusQueue: []
    property var polled: []

    function initTestCase() {
        Qt.application.organization = "auctions-qml-tests"
        Qt.application.domain = "auctions.test"
    }

    QtObject {
        id: backendStub
        property string activeAccount: "owner"
        function txStatus(id) { testCase.polled.push(id); return { call: "txStatus", txId: id } }
    }

    QtObject {
        id: logos
        function module(name) { return backendStub }
        function watch(promise, ok, err) {
            if (!promise || promise.call !== "txStatus")
                return
            const next = testCase.statusQueue.length > 0
                ? testCase.statusQueue.shift()
                : { txId: promise.txId, state: "pending", blocks_waited: 1, step: 0, steps: 1 }
            ok(next)
        }
    }

    Component {
        id: laneComponent
        TxLane {
            width: 420
            backend: backendStub
        }
    }

    Component {
        id: dialogComponent
        TxDialog {}
    }

    SignalSpy { id: confirmedSpy; signalName: "confirmed" }
    SignalSpy { id: rejectedSpy; signalName: "rejected" }
    SignalSpy { id: requoteSpy; signalName: "requote" }

    function init() {
        statusQueue = []
        polled = []
        backendStub.activeAccount = "owner"
    }

    function bidRequest() {
        const quote = Fixtures.load("quote_bid")
        return {
            action: "bid",
            auction: quote.auction,
            coin_in_max: quote.coin_in_max,
            from: "7wx93ACHrg2tR8vHRBcKnVkRJTrtT7zcP3UsxcViopKg",
            pay: { label: "You pay", amount: quote.coin_in_max, decimals: 0, unit: "stable" },
            get: { label: "You get", amount: quote.collateral_out, decimals: 0, unit: "collateral",
                   note: "minimum " + quote.min_out + " · 0.5% slippage" },
            facts: [
                { label: "Debt part", value: quote.debt_part, decimals: 0 },
                { label: "Penalty part", value: quote.penalty_part, decimals: 0 },
                { label: "Collateral left after", value: quote.collateral_left, decimals: 0 }
            ]
        }
    }

    function openDialog() {
        const dialog = createTemporaryObject(dialogComponent, testCase,
                                             { request: bidRequest(), quote: Fixtures.load("quote_bid") })
        verify(dialog !== null)
        dialog.open()
        tryCompare(dialog, "opened", true)
        return dialog
    }

    // ---- TxDialog -------------------------------------------------------------------------------

    function test_dialog_takes_title_and_rows_from_the_request() {
        const dialog = openDialog()
        compare(findChild(dialog.header, "txTitle").text, "Bid on auction")
        compare(findChild(dialog.contentItem, "txPayAmount").text, "100,000")
        compare(findChild(dialog.contentItem, "txGetAmount").text, "≈ 51,963")
        const rows = findChild(dialog.contentItem, "txFacts")
        compare(rows.count, 3)
        compare(findChild(rows, "quoteRowValue_1").text, "0")
        compare(findChild(rows, "quoteRowValue_2").text, "475,000")
        compare(findChild(dialog.contentItem, "txPrivate").text, "Nothing. Private bids are later work (M4).")
    }

    function test_dialog_focuses_cancel_and_escape_cancels() {
        const dialog = openDialog()
        confirmedSpy.target = dialog
        rejectedSpy.target = dialog
        confirmedSpy.clear()
        rejectedSpy.clear()
        const cancel = findChild(dialog.footer, "txCancel")
        tryVerify(function() { return cancel.activeFocus }, 1000, "Cancel has focus on open")
        keyClick(Qt.Key_Escape)
        tryCompare(dialog, "visible", false)
        compare(rejectedSpy.count, 1)
        compare(confirmedSpy.count, 0)
    }

    function test_dialog_confirm_emits_confirmed() {
        const dialog = openDialog()
        confirmedSpy.target = dialog
        confirmedSpy.clear()
        findChild(dialog.footer, "txConfirm").clicked()
        compare(confirmedSpy.count, 1)
        tryCompare(dialog, "visible", false)
    }

    function test_dialog_without_trade_hides_the_swap() {
        const dialog = createTemporaryObject(dialogComponent, testCase, {
            request: { action: "settle_debt", facts: [{ label: "Covers", value: "1549", decimals: 0 }] },
            quote: Fixtures.load("quote_settle_debt")
        })
        dialog.open()
        tryCompare(dialog, "opened", true)
        compare(findChild(dialog.header, "txTitle").text, "Settle debt")
        verify(!findChild(dialog.contentItem, "txSwap").visible)
        compare(findChild(dialog.contentItem, "txFacts").count, 1)
    }

    // ---- TxLane ---------------------------------------------------------------------------------

    function createLane(props) {
        const lane = createTemporaryObject(laneComponent, testCase, props)
        verify(lane !== null)
        return lane
    }

    function test_lane_survives_account_switch() {
        const lane = createLane({ txId: "tx1" })
        compare(lane.signer, "owner", "signer captured when the transaction starts")
        compare(lane.state, "waiting")

        backendStub.activeAccount = "keeper"
        compare(lane.signer, "owner", "switching account does not re-sign the lane")
        compare(lane.state, "waiting")
        lane.poll()
        compare(polled[polled.length - 1], "tx1", "still polls its own transaction")
        compare(lane.state, "waiting")

        statusQueue = [{ txId: "tx1", state: "included", step: 0, steps: 1, nonce_after: "4" }]
        lane.poll()
        compare(lane.state, "done")
        compare(lane.signer, "owner")
        verify(findChild(lane, "txLaneSigner").text.indexOf("owner") >= 0)
    }

    // Review Focus 1: the node stops answering mid-lane. Never "Dropped".
    function test_lane_shows_unknown_not_dropped() {
        const lane = createLane({ txId: "tx2" })
        statusQueue = [{ error: "the node did not answer within 5 s", kind: "chain" }]
        lane.poll()
        compare(lane.state, "unknown")
        compare(findChild(lane, "txLaneTitle").text, "Outcome unknown, lock kept: check the nonce")
        verify(!findChild(lane, "txRequote").visible, "no Re-quote while the outcome is unknown")
        const lanes = findChild(lane, "txStages")
        verify(!lanes.failed, "unknown is not drawn as failed")
        verify(findChild(lane, "txLaneTitle").text.toLowerCase().indexOf("drop") < 0)
        verify(findChild(lane, "txLaneBody").text.toLowerCase().indexOf("dropped") < 0)

        // The engine still tracks it: once the node answers, the lane resolves.
        statusQueue = [{ txId: "tx2", state: "included", step: 0, steps: 1 }]
        lane.poll()
        compare(lane.state, "done")
    }

    function test_lane_sequence_end_error_is_unknown_and_stops() {
        const lane = createLane({ txId: "tx3" })
        statusQueue = [{ error: "lez_core did not answer the send: it may or may not have reached the node",
                         kind: "send", step: 1, includedSteps: 1, discardedSteps: 1 }]
        lane.poll()
        compare(lane.state, "unknown")
        verify(!lane.polling, "a sequence that ended is not polled again")
    }

    function test_lane_dropped_offers_requote() {
        const lane = createLane({ txId: "tx4" })
        requoteSpy.target = lane
        requoteSpy.clear()
        statusQueue = [{ txId: "tx4", state: "pending", blocks_waited: 3, step: 0, steps: 1 },
                       { txId: "tx4", state: "dropped", height: 12, step: 0, steps: 1,
                         includedSteps: 0, discardedSteps: 0 }]
        lane.poll()
        compare(findChild(lane, "txLaneCaption").text, "block 3 of 5")
        lane.poll()
        compare(lane.state, "dropped")
        compare(findChild(lane, "txLaneTitle").text, "Not included after 5 blocks")
        verify(findChild(lane, "txStages").failed)
        const requote = findChild(lane, "txRequote")
        verify(requote.visible)
        requote.clicked()
        compare(requoteSpy.count, 1)
        verify(!lane.polling)
    }

    function test_lane_multi_step_shows_one_stage_per_step() {
        const lane = createLane({ txId: "tx5", steps: 3,
                                  stepLabels: ["Observe feed", "Publish", "Set params"] })
        const stages = findChild(lane, "txStages")
        compare(stages.count, 3)
        compare(stages.currentIndex, 0)
        statusQueue = [{ txId: "tx5", state: "next", step: 1, steps: 3 }]
        lane.poll()
        compare(stages.currentIndex, 1)
        compare(lane.state, "waiting")
        statusQueue = [{ txId: "tx5", state: "next", step: 2, steps: 3 },
                       { txId: "tx5", state: "included", step: 2, steps: 3 }]
        lane.poll()
        lane.poll()
        compare(lane.state, "done")
        compare(stages.currentIndex, 3)
    }

    function test_lane_polls_every_two_seconds() {
        const lane = createLane({ txId: "tx6" })
        compare(lane.pollInterval, 2000)
        tryVerify(function() { return testCase.polled.length >= 1 }, 3000)
    }
}
