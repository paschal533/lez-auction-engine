import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common/Format.js" as Format

// Progress of one submitted transaction (board v4 "After Confirm bid"), shown in place of the action button.
// Set `submitReply` (the submit reply: txId, steps, tracking) and `txId` from it, plus `stepLabels` for a
// multi-step action; the lane polls backend.txStatus every 2 s, one call at a time, until the outcome is known.
// One lane may be reused: a new txId resets it. `state`:
//   waiting   accepted by the node; included once the signer's nonce advances
//   done      every step included
//   dropped   a step was not included within the drop window, or a later step was not sent (module
//             outcome `dropped` / `not_sent`); Re-quote emits requote()
//   unknown   the engine could not tell (node not answering, a send whose fate is unknown, no outcome): the
//             signer's lock is kept and the lane never says "Dropped" (Review Focus 1). While the engine still
//             tracks the transaction the lane keeps polling and resolves when the node answers again.
// `signer` is the account that signed: the submit reply's tracking.signer (or request.from), shown by its
// wallet label from `accounts`. It never follows the active account.
ColumnLayout {
    id: root
    objectName: root.scoped("txLane")

    // Ruling R34: a screen that hosts its own lane next to another one's prefixes every objectName
    // ("risk" -> riskTxLane, riskTxRequote, ...), so the e2e runner finds each by a unique name. Empty
    // keeps the plain names (txLane, txRequote, ...).
    property string namePrefix: ""

    function scoped(base) {
        return namePrefix.length > 0 ? namePrefix + base.charAt(0).toUpperCase() + base.substring(1) : base
    }

    property var backend: null
    property string txId: ""
    property var submitReply: null
    // The request that was submitted; `from` names the signer when the reply carries no tracking.
    property var request: null
    // auction_engine accounts() rows, to show the signer's label rather than its id.
    property var accounts: []
    property int steps: 1
    // One label per step of a multi-step action, a string or {label, busyLabel}. Default "Step N".
    property var stepLabels: []
    readonly property int pollInterval: 2000
    readonly property bool polling: pollTimer.running
    readonly property string signer: d.labelFor(d.signerId)
    // What the engine says the drop window is (R29); the SDK default until a reply carries it.
    readonly property int dropAfterBlocks: d.dropAfterBlocks

    signal requote()

    function poll() {
        // One txStatus call at a time: a slow reply must not pile up calls behind it.
        if (!root.backend || root.txId.length === 0 || d.inFlight)
            return
        const id = root.txId
        d.inFlight = true
        logos.watch(root.backend.txStatus(id), function(reply) {
            if (id !== root.txId) return
            d.inFlight = false
            d.apply(reply)
        }, function(err) {
            if (id !== root.txId) return
            d.inFlight = false
            d.markUnknown(String(err), true)
        })
    }

    spacing: Theme.spacing.medium
    state: ""

    states: [
        State { name: "waiting" },
        State { name: "done" },
        State { name: "dropped" },
        State { name: "unknown" }
    ]

    onTxIdChanged: d.start()
    onSubmitReplyChanged: d.takeSubmitReply()
    onStepsChanged: d.rebuildStages()
    onStepLabelsChanged: d.rebuildStages()
    Component.onCompleted: {
        d.rebuildStages()
        d.start()
    }

    QtObject {
        id: d

        readonly property int defaultDropAfterBlocks: 5

        // The step in flight (0-based) and what the engine last said about it.
        property int step: 0
        property int blocksWaited: -1
        property int includedSteps: 0
        property int discardedSteps: 0
        property int dropAfterBlocks: defaultDropAfterBlocks
        // `dropped` came from a step that was not sent (an error with outcome dropped / not_sent).
        property bool notSent: false
        property string detail: ""
        property string signerId: ""
        property bool inFlight: false
        readonly property bool multi: root.steps > 1

        function start() {
            step = 0
            blocksWaited = -1
            includedSteps = 0
            discardedSteps = 0
            dropAfterBlocks = defaultDropAfterBlocks
            notSent = false
            detail = ""
            signerId = ""
            inFlight = false
            if (root.txId.length === 0) {
                root.state = ""
                pollTimer.stop()
                return
            }
            takeSubmitReply()
            root.state = "waiting"
            pollTimer.restart()
        }

        // The submit reply of this txId names the signer and the step count.
        function takeSubmitReply() {
            const reply = root.submitReply
            if (reply && reply.txId === root.txId && root.txId.length > 0) {
                if (reply.steps !== undefined && Number(reply.steps) > 0)
                    root.steps = Number(reply.steps)
                if (reply.tracking && reply.tracking.signer) {
                    signerId = String(reply.tracking.signer)
                    return
                }
            }
            if (signerId.length === 0 && root.txId.length > 0 && root.request && root.request.from)
                signerId = String(root.request.from)
        }

        function labelFor(id) {
            if (!id) return ""
            const rows = root.accounts || []
            for (let i = 0; i < rows.length; i++)
                if (rows[i] && (rows[i].id === id || rows[i].label === id)) return String(rows[i].label)
            return Format.shortId(id)
        }

        function takeStep(reply) {
            if (reply.steps !== undefined && Number(reply.steps) > 0 && Number(reply.steps) !== root.steps)
                root.steps = Number(reply.steps)
            if (reply.step !== undefined)
                step = Number(reply.step)
            if (reply.includedSteps !== undefined)
                includedSteps = Number(reply.includedSteps)
            if (reply.discardedSteps !== undefined)
                discardedSteps = Number(reply.discardedSteps)
            if (reply.drop_after_blocks !== undefined && Number(reply.drop_after_blocks) > 0)
                dropAfterBlocks = Number(reply.drop_after_blocks)
        }

        function apply(reply) {
            if (!reply) {
                markUnknown(qsTr("The engine sent no reply."), true)
                return
            }
            if (reply.error) {
                applyError(reply)
                return
            }
            takeStep(reply)
            detail = ""
            switch (reply.state) {
            case "pending":
                blocksWaited = reply.blocks_waited !== undefined ? Number(reply.blocks_waited) : blocksWaited
                root.state = "waiting"
                break
            case "next":
                blocksWaited = -1
                root.state = "waiting"
                break
            case "included":
                root.state = "done"
                pollTimer.stop()
                break
            case "dropped":
                notSent = false
                root.state = "dropped"
                pollTimer.stop()
                break
            default:
                markUnknown(qsTr("The engine reported the state \"%1\".").arg(reply.state), true)
            }
        }

        // R27: a reply that ends the sequence (it carries includedSteps) says what became of the step in
        // `outcome`. dropped / not_sent: nothing of that step reached the node. unknown, or no outcome: the
        // fate is not known. A transaction the engine no longer knows also ends here. Anything else (a
        // `chain` read failure) leaves the transaction tracked: keep polling.
        function applyError(reply) {
            const ended = reply.includedSteps !== undefined || reply.kind === "unknown_tx"
            if (!ended) {
                markUnknown(String(reply.error), true)
                return
            }
            takeStep(reply)
            if (reply.outcome === "dropped" || reply.outcome === "not_sent") {
                notSent = true
                detail = String(reply.error)
                root.state = "dropped"
                pollTimer.stop()
                return
            }
            markUnknown(String(reply.error), false)
        }

        function markUnknown(message, keepPolling) {
            detail = message
            root.state = "unknown"
            if (!keepPolling)
                pollTimer.stop()
        }

        function labelAt(i) {
            const entry = root.stepLabels && i < root.stepLabels.length ? root.stepLabels[i] : null
            if (entry && typeof entry === "object")
                return { label: String(entry.label || ""), busyLabel: String(entry.busyLabel || "") }
            const label = entry ? String(entry) : qsTr("Step %1").arg(i + 1)
            return { label: label, busyLabel: label + "…" }
        }

        function rebuildStages() {
            const old = []
            for (let i = 0; i < lane.stages.length; i++)
                old.push(lane.stages[i])
            const fresh = []
            if (root.steps > 1) {
                for (let i = 0; i < root.steps; i++)
                    fresh.push(stageComponent.createObject(root, labelAt(i)))
            } else {
                fresh.push(stageComponent.createObject(root, { label: qsTr("Submitted") }))
                fresh.push(stageComponent.createObject(root, { label: qsTr("Included"),
                                                              busyLabel: qsTr("Including…") }))
                fresh.push(stageComponent.createObject(root, { label: qsTr("Done") }))
            }
            lane.stages = fresh
            for (let i = 0; i < old.length; i++)
                old[i].destroy()
        }

        // Single step: Submitted (done) → Included (busy) → Done. Multi-step: one stage per step.
        readonly property int laneIndex: {
            if (root.state === "done")
                return multi ? root.steps : 2
            return multi ? step : 1
        }

        // "Steps 1–k are on chain" (k = includedSteps), or "Step 1 is on chain".
        readonly property string onChain: includedSteps === 1
            ? qsTr("Step 1 is on chain")
            : qsTr("Steps 1–%1 are on chain").arg(includedSteps)

        readonly property string title: {
            switch (root.state) {
            case "done": return multi ? qsTr("All %1 steps included").arg(root.steps) : qsTr("Included")
            case "dropped":
                return notSent ? qsTr("Step %1 not sent: %2").arg(step + 1).arg(detail)
                               : qsTr("Not included after %1 blocks").arg(dropAfterBlocks)
            case "unknown": return qsTr("Outcome unknown, lock kept: check the nonce")
            case "waiting":
                return multi ? qsTr("Step %1 of %2: %3").arg(step + 1).arg(root.steps).arg(labelAt(step).label)
                             : qsTr("Waiting to be included")
            default: return ""
            }
        }

        readonly property string caption: root.state === "waiting" && blocksWaited >= 0
            ? qsTr("block %1 of %2").arg(blocksWaited).arg(dropAfterBlocks) : ""

        readonly property string body: {
            switch (root.state) {
            case "waiting":
                return multi ? qsTr("Each step waits for its own nonce; the next one is sent once this one is included.")
                             : qsTr("Accepted by the node. Included once your signer nonce advances.")
            case "done":
                return qsTr("On chain.")
            case "dropped": {
                const verb = notSent ? qsTr("was not sent") : qsTr("was not included")
                if (includedSteps > 0) {
                    const later = discardedSteps > 0 ? " " + qsTr("The %1 step(s) after it were not sent.").arg(discardedSteps) : ""
                    return qsTr("%1; step %2 %3.").arg(onChain).arg(includedSteps + 1).arg(verb) + later
                }
                return notSent
                    ? qsTr("Nothing was sent, nothing was spent.")
                    : qsTr("The transaction was dropped, nothing was spent. Prices may have moved since the quote.")
            }
            case "unknown":
                return detail.length > 0 ? detail : qsTr("The engine could not tell whether it was included.")
            default:
                return ""
            }
        }
    }

    Component {
        id: stageComponent
        LogosStage {}
    }

    Timer {
        id: pollTimer
        objectName: root.scoped("txPollTimer")
        interval: root.pollInterval
        repeat: true
        // Only while the outcome is open (waiting, or unknown while the engine still tracks it): a tick that lands
        // after done or dropped polls nothing.
        onTriggered: if (root.state === "waiting" || root.state === "unknown") root.poll()
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: Theme.spacing.small

        LogosText {
            objectName: root.scoped("txLaneTitle")
            Layout.fillWidth: true
            text: d.title
            wrapMode: Text.WordWrap
            font.pixelSize: Theme.typography.primaryText
            font.weight: Theme.typography.weightMedium
            color: root.state === "dropped" ? Theme.palette.error
                 : root.state === "unknown" ? Theme.palette.warning
                 : Theme.palette.text
        }
        LogosText {
            objectName: root.scoped("txLaneCaption")
            visible: text.length > 0
            text: d.caption
            font.pixelSize: Theme.typography.secondaryText
            font.features: { "tnum": 1 }
            color: Theme.palette.textTertiary
        }
    }

    LogosStageLane {
        id: lane
        objectName: root.scoped("txStages")
        Layout.fillWidth: true
        currentIndex: root.state === "" ? -1 : d.laneIndex
        busy: root.state === "waiting" || root.state === "unknown"
        failed: root.state === "dropped"
        // Unknown holds the stage in the warning colour without the pulse: nothing is happening we can see.
        pulseDuration: root.state === "unknown" ? 0 : 2800
    }

    LogosText {
        objectName: root.scoped("txLaneBody")
        Layout.fillWidth: true
        text: d.body
        wrapMode: Text.WordWrap
        font.pixelSize: Theme.typography.secondaryText
        color: Theme.palette.textSecondary
    }

    LogosText {
        objectName: root.scoped("txLaneSigner")
        Layout.fillWidth: true
        visible: root.signer.length > 0
        text: qsTr("Signed as %1").arg(root.signer)
        font.pixelSize: Theme.typography.secondaryText
        color: Theme.palette.textTertiary
    }

    LogosButton {
        id: requoteButton
        objectName: root.scoped("txRequote")
        visible: root.state === "dropped"
        text: qsTr("Re-quote")
        Layout.preferredHeight: 40
        radius: Theme.spacing.radiusLarge
        variant: LogosButton.Variant.Primary
        onClicked: root.requote()
        // Primary buttons carry a black label (contrast on the orange fill).
        Component.onCompleted: labelItem.color = Qt.binding(function() {
            return enabled ? Theme.palette.backgroundBlack : Theme.palette.textMuted
        })
    }
}
