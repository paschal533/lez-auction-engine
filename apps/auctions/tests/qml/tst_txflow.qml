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
    // Date.now() at each txStatus call, for the poll-interval test.
    property var pollTimes: []
    // With deferWatch the stub holds txStatus replies back; the test answers them through `deferred`.
    property bool deferWatch: false
    property var deferred: []

    function initTestCase() {
        Qt.application.organization = "auctions-qml-tests"
        Qt.application.domain = "auctions.test"
    }

    QtObject {
        id: backendStub
        property string activeAccount: "owner"
        function txStatus(id) {
            testCase.polled.push(id)
            testCase.pollTimes.push(Date.now())
            return { call: "txStatus", txId: id }
        }
    }

    QtObject {
        id: logos
        function module(name) { return backendStub }
        function watch(promise, ok, err) {
            if (!promise || promise.call !== "txStatus")
                return
            if (testCase.deferWatch) {
                testCase.deferred.push(ok)
                return
            }
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
        pollTimes = []
        deferWatch = false
        deferred = []
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

    readonly property string ownerId: "7wx93ACHrg2tR8vHRBcKnVkRJTrtT7zcP3UsxcViopKg"
    readonly property string keeperId: "5HSaTMUz6UMtMRwx8yH7Z9dHWLNF9G4op6dh8DM5KKeE"

    function submitReply(txId, signerId, steps) {
        return { txId: txId, hash: "ab", what: "bid", step: 0, steps: steps || 1,
                 tracking: { signer: signerId, nonce_before: "0", height_before: 10 } }
    }

    function createLane(props) {
        const lane = createTemporaryObject(laneComponent, testCase, props)
        verify(lane !== null)
        return lane
    }

    function laneFor(txId, signerId, steps, extra) {
        const props = { accounts: Fixtures.load("accounts"), submitReply: submitReply(txId, signerId, steps),
                        txId: txId }
        for (const k in (extra || {})) props[k] = extra[k]
        return createLane(props)
    }

    function test_lane_survives_account_switch() {
        const lane = laneFor("tx1", ownerId)
        compare(lane.signer, "owner", "signer is the submit reply's tracking.signer, by its label")
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

    function test_lane_signer_never_comes_from_the_active_account() {
        backendStub.activeAccount = "taker"
        const fromRequest = createLane({ accounts: Fixtures.load("accounts"), request: { from: keeperId },
                                         txId: "tx7" })
        compare(fromRequest.signer, "keeper", "falls back to request.from")
        const none = createLane({ txId: "tx8" })
        compare(none.signer, "", "no reply, no request: no signer, not the active account")
    }

    function test_lane_is_reset_for_a_second_txid() {
        const lane = laneFor("tx1", ownerId)
        statusQueue = [{ txId: "tx1", state: "pending", blocks_waited: 3, step: 0, steps: 1, drop_after_blocks: 8 },
                       { txId: "tx1", state: "dropped", height: 20, step: 0, steps: 1, drop_after_blocks: 8,
                         outcome: "dropped", includedSteps: 0, discardedSteps: 0 }]
        lane.poll()
        lane.poll()
        compare(lane.state, "dropped")
        verify(!lane.polling)

        lane.submitReply = submitReply("tx2", keeperId)
        lane.txId = "tx2"
        compare(lane.state, "waiting")
        compare(lane.signer, "keeper")
        compare(lane.dropAfterBlocks, 5, "the drop window falls back to 5 until a reply says otherwise")
        compare(findChild(lane, "txLaneCaption").text, "")
        verify(lane.polling)
        lane.poll()
        compare(polled[polled.length - 1], "tx2")
    }

    // Review Focus 1: the node stops answering mid-lane. Never "Dropped".
    function test_lane_shows_unknown_not_dropped() {
        const lane = laneFor("tx2", ownerId)
        statusQueue = [{ error: "the node did not answer within 5 s", kind: "chain" }]
        lane.poll()
        compare(lane.state, "unknown")
        compare(findChild(lane, "txLaneTitle").text, "Outcome unknown, lock kept: check the nonce")
        verify(!findChild(lane, "txRequote").visible, "no Re-quote while the outcome is unknown")
        verify(!findChild(lane, "txStages").failed, "unknown is not drawn as failed")
        verify(findChild(lane, "txLaneTitle").text.toLowerCase().indexOf("drop") < 0)
        verify(findChild(lane, "txLaneBody").text.toLowerCase().indexOf("dropped") < 0)
        verify(lane.polling, "the engine still tracks it: keep polling")

        statusQueue = [{ txId: "tx2", state: "included", step: 0, steps: 1 }]
        lane.poll()
        compare(lane.state, "done")
    }

    // R27: the module says what became of a later step whose begin or send failed.
    function sequenceEnd(outcome) {
        const reply = { error: "refused", kind: outcome === "not_sent" ? "in_flight" : "send",
                        step: 1, includedSteps: 1, discardedSteps: 1 }
        if (outcome !== undefined) reply.outcome = outcome
        return reply
    }

    function multiLane(txId) {
        return laneFor(txId, ownerId, 3, { stepLabels: ["Observe feed", "Publish", "Set params"] })
    }

    function test_lane_outcome_unknown_is_unknown_and_stops() {
        const lane = multiLane("tx3")
        statusQueue = [sequenceEnd("unknown")]
        lane.poll()
        compare(lane.state, "unknown")
        verify(!findChild(lane, "txRequote").visible)
        verify(!lane.polling, "a sequence that ended is not polled again")
    }

    function test_lane_outcome_dropped_is_a_step_not_sent() {
        const lane = multiLane("tx3")
        statusQueue = [sequenceEnd("dropped")]
        lane.poll()
        compare(lane.state, "dropped")
        compare(findChild(lane, "txLaneTitle").text, "Step 2 not sent: refused")
        verify(findChild(lane, "txRequote").visible)
        verify(!lane.polling)
    }

    function test_lane_outcome_not_sent_is_a_step_not_sent() {
        const lane = multiLane("tx3")
        statusQueue = [sequenceEnd("not_sent")]
        lane.poll()
        compare(lane.state, "dropped")
        compare(findChild(lane, "txLaneTitle").text, "Step 2 not sent: refused")
        // Step 1 is on chain: the lane must not claim nothing was spent.
        const body = findChild(lane, "txLaneBody").text
        verify(body.indexOf("nothing was spent") < 0, body)
        verify(body.indexOf("Step 1 is on chain; step 2 was not sent.") === 0, body)
        verify(findChild(lane, "txRequote").visible)
    }

    function test_lane_missing_outcome_is_unknown() {
        const lane = multiLane("tx3")
        statusQueue = [sequenceEnd(undefined)]
        lane.poll()
        compare(lane.state, "unknown")
        verify(!findChild(lane, "txRequote").visible)
        verify(!lane.polling)
    }

    function test_lane_dropped_offers_requote() {
        const lane = laneFor("tx4", ownerId)
        requoteSpy.target = lane
        requoteSpy.clear()
        statusQueue = [{ txId: "tx4", state: "pending", blocks_waited: 3, step: 0, steps: 1 },
                       { txId: "tx4", state: "dropped", height: 12, step: 0, steps: 1, outcome: "dropped",
                         includedSteps: 0, discardedSteps: 0 }]
        lane.poll()
        compare(findChild(lane, "txLaneCaption").text, "block 3 of 5")
        lane.poll()
        compare(lane.state, "dropped")
        compare(findChild(lane, "txLaneTitle").text, "Not included after 5 blocks")
        verify(findChild(lane, "txLaneBody").text.indexOf("nothing was spent") >= 0)
        verify(findChild(lane, "txStages").failed)
        const requote = findChild(lane, "txRequote")
        verify(requote.visible)
        requote.clicked()
        compare(requoteSpy.count, 1)
        verify(!lane.polling)
    }

    // R29: the drop window comes from the engine.
    function test_lane_uses_the_engines_drop_window() {
        const lane = laneFor("tx9", ownerId)
        statusQueue = [{ txId: "tx9", state: "pending", blocks_waited: 3, step: 0, steps: 1, drop_after_blocks: 8 },
                       { txId: "tx9", state: "dropped", height: 18, step: 0, steps: 1, drop_after_blocks: 8,
                         outcome: "dropped", includedSteps: 0, discardedSteps: 0 }]
        lane.poll()
        compare(findChild(lane, "txLaneCaption").text, "block 3 of 8")
        lane.poll()
        compare(findChild(lane, "txLaneTitle").text, "Not included after 8 blocks")
    }

    function test_lane_dropped_later_step_says_what_is_on_chain() {
        const lane = multiLane("tx10")
        statusQueue = [{ txId: "tx10", state: "next", step: 1, steps: 3 },
                       { txId: "tx10", state: "next", step: 2, steps: 3 },
                       { txId: "tx10", state: "dropped", height: 30, step: 2, steps: 3, outcome: "dropped",
                         includedSteps: 2, discardedSteps: 0 }]
        lane.poll()
        lane.poll()
        lane.poll()
        compare(lane.state, "dropped")
        compare(findChild(lane, "txLaneTitle").text, "Not included after 5 blocks")
        const body = findChild(lane, "txLaneBody").text
        compare(body, "Steps 1–2 are on chain; step 3 was not included.")
    }

    function test_lane_multi_step_shows_one_stage_per_step() {
        const lane = multiLane("tx5")
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

    function test_lane_sends_one_txstatus_at_a_time() {
        const lane = laneFor("tx11", ownerId)
        deferWatch = true
        lane.poll()
        lane.poll()
        lane.poll()
        compare(polled.length, 1, "no second call while the first is out")
        compare(deferred.length, 1)
        deferred.shift()({ txId: "tx11", state: "pending", blocks_waited: 1, step: 0, steps: 1 })
        lane.poll()
        compare(polled.length, 2, "the reply clears the guard")
        deferred.shift()({ txId: "tx11", state: "included", step: 0, steps: 1 })
        compare(lane.state, "done")
        deferWatch = false
    }

    // Measured from the stub's own timestamps (R41), so a slow event loop elsewhere in the run cannot stretch
    // the measurement: the first poll comes no sooner than 2 s after the lane starts, the next one about 2 s later
    // (one call at a time, a fixed 2 s timer), and none after the outcome is known.
    function test_lane_polls_every_two_seconds() {
        const laneStart = Date.now()
        const lane = laneFor("tx6", ownerId)
        compare(lane.pollInterval, 2000)
        tryVerify(function() { return testCase.pollTimes.length >= 2 }, 8000)
        const first = pollTimes[0] - laneStart
        const gap = pollTimes[1] - pollTimes[0]
        verify(first >= 1800, "first poll after about 2 s, was " + first + " ms")
        verify(gap >= 1800 && gap <= 2600, "second poll about 2 s after the first, was " + gap + " ms")

        statusQueue = [{ txId: "tx6", state: "included", step: 0, steps: 1 }]
        tryCompare(lane, "state", "done", 5000)
        verify(!lane.polling)
        const count = polled.length
        wait(2500)
        compare(polled.length, count, "no polls once done")
    }
}
