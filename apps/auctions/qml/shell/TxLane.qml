import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

// Progress of one submitted transaction (board v4 "After Confirm bid"), shown in place of the action button.
// Set `txId` from the submit reply (and `steps` / `stepLabels` for a multi-step action); the lane polls
// backend.txStatus every 2 s until the outcome is known. `state`:
//   waiting   accepted by the node; included once the signer's nonce advances
//   done      every step included
//   dropped   not included within the drop window: nothing was spent; Re-quote emits requote()
//   unknown   the engine could not tell (node not answering, a send whose fate is unknown): the signer's
//             lock is kept and the lane never says "Dropped" (Review Focus 1). While the engine still tracks
//             the transaction the lane keeps polling and resolves when the node answers again.
// `signer` is the account that signed, captured when the transaction starts: switching the active account
// later does not change it.
ColumnLayout {
    id: root
    objectName: "txLane"

    property var backend: null
    property string txId: ""
    property string signer: ""
    property int steps: 1
    // One label per step of a multi-step action, a string or {label, busyLabel}. Default "Step N".
    property var stepLabels: []
    readonly property int pollInterval: 2000
    readonly property bool polling: pollTimer.running

    // The SDK's ConfirmSettings::drop_after_blocks: a transaction whose signer nonce has not moved this many
    // blocks after submit is dropped.
    readonly property int dropAfterBlocks: 5

    signal requote()

    function poll() {
        if (!root.backend || root.txId.length === 0)
            return
        const id = root.txId
        logos.watch(root.backend.txStatus(id), function(reply) {
            if (id === root.txId) d.apply(reply)
        }, function(err) {
            if (id === root.txId) d.markUnknown(String(err), true)
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
    onStepsChanged: d.rebuildStages()
    onStepLabelsChanged: d.rebuildStages()
    Component.onCompleted: {
        d.rebuildStages()
        d.start()
    }

    QtObject {
        id: d

        // The step in flight (0-based) and what the engine last said about it.
        property int step: 0
        property int blocksWaited: -1
        property int discardedSteps: 0
        property string detail: ""
        readonly property bool multi: root.steps > 1

        function start() {
            step = 0
            blocksWaited = -1
            discardedSteps = 0
            detail = ""
            if (root.txId.length === 0) {
                root.state = ""
                pollTimer.stop()
                return
            }
            // One-time copy, not a binding: the lane keeps the account that signed.
            if (root.signer.length === 0 && root.backend && root.backend.activeAccount)
                root.signer = String(root.backend.activeAccount)
            root.state = "waiting"
            pollTimer.restart()
        }

        function takeStep(reply) {
            if (reply.steps !== undefined && Number(reply.steps) > 0 && Number(reply.steps) !== root.steps)
                root.steps = Number(reply.steps)
            if (reply.step !== undefined)
                step = Number(reply.step)
        }

        function apply(reply) {
            if (!reply) {
                markUnknown(qsTr("The engine sent no reply."), true)
                return
            }
            if (reply.error) {
                // A reply that ends the sequence (it carries includedSteps), or a transaction the engine no
                // longer knows, will not resolve by polling again. Anything else (a `chain` read failure)
                // leaves the transaction tracked: keep polling.
                const ended = reply.includedSteps !== undefined || reply.kind === "unknown_tx"
                if (ended) takeStep(reply)
                markUnknown(String(reply.error), !ended)
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
                discardedSteps = reply.discardedSteps !== undefined ? Number(reply.discardedSteps) : 0
                root.state = "dropped"
                pollTimer.stop()
                break
            default:
                markUnknown(qsTr("The engine reported the state \"%1\".").arg(reply.state), true)
            }
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

        readonly property string title: {
            switch (root.state) {
            case "done": return multi ? qsTr("All %1 steps included").arg(root.steps) : qsTr("Included")
            case "dropped": return qsTr("Not included after %1 blocks").arg(root.dropAfterBlocks)
            case "unknown": return qsTr("Outcome unknown, lock kept: check the nonce")
            case "waiting":
                return multi ? qsTr("Step %1 of %2: %3").arg(step + 1).arg(root.steps).arg(labelAt(step).label)
                             : qsTr("Waiting to be included")
            default: return ""
            }
        }

        readonly property string caption: root.state === "waiting" && blocksWaited >= 0
            ? qsTr("block %1 of %2").arg(blocksWaited).arg(root.dropAfterBlocks) : ""

        readonly property string body: {
            switch (root.state) {
            case "waiting":
                return multi ? qsTr("Each step waits for its own nonce; the next one is sent once this one is included.")
                             : qsTr("Accepted by the node. Included once your signer nonce advances.")
            case "done":
                return qsTr("On chain.")
            case "dropped":
                return discardedSteps > 0
                    ? qsTr("The transaction was dropped, nothing was spent. The %1 later step(s) were not sent.").arg(discardedSteps)
                    : qsTr("The transaction was dropped, nothing was spent. Prices may have moved since the quote.")
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
        interval: root.pollInterval
        repeat: true
        onTriggered: root.poll()
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: Theme.spacing.small

        LogosText {
            objectName: "txLaneTitle"
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
            objectName: "txLaneCaption"
            visible: text.length > 0
            text: d.caption
            font.pixelSize: Theme.typography.secondaryText
            font.features: { "tnum": 1 }
            color: Theme.palette.textTertiary
        }
    }

    LogosStageLane {
        id: lane
        objectName: "txStages"
        Layout.fillWidth: true
        currentIndex: root.state === "" ? -1 : d.laneIndex
        busy: root.state === "waiting" || root.state === "unknown"
        failed: root.state === "dropped"
        // Unknown holds the stage in the warning colour without the pulse: nothing is happening we can see.
        pulseDuration: root.state === "unknown" ? 0 : 2800
    }

    LogosText {
        objectName: "txLaneBody"
        Layout.fillWidth: true
        text: d.body
        wrapMode: Text.WordWrap
        font.pixelSize: Theme.typography.secondaryText
        color: Theme.palette.textSecondary
    }

    LogosText {
        objectName: "txLaneSigner"
        Layout.fillWidth: true
        visible: root.signer.length > 0
        text: qsTr("Signed as %1").arg(root.signer)
        font.pixelSize: Theme.typography.secondaryText
        color: Theme.palette.textTertiary
    }

    LogosButton {
        id: requoteButton
        objectName: "txRequote"
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
