import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common/Format.js" as Format

// Dev tools > Token transfer: moves tokens between two wallet accounts (for example to fund a taker
// before a debt take). The engine refuses the request unless the network is a verified dev network.
Rectangle {
    id: root

    property var backend: null

    readonly property bool amountValid: /^[0-9]+$/.test(amountField.text)
    readonly property bool canSubmit: !d.busy
                                      && fromField.text.length > 0
                                      && toField.text.length > 0
                                      && amountValid

    color: Theme.palette.surfaceRaised
    radius: Theme.spacing.radiusXlarge

    QtObject {
        id: d

        readonly property int pollIntervalMs: 2000

        property bool busy: false
        property bool failed: false
        property string status: ""
        // A reply for a request older than the current one is dropped.
        property int generation: 0
        property string txId: ""

        function finish(message, isFailure) {
            busy = false
            failed = isFailure
            status = message
            pollTimer.stop()
        }

        function submit() {
            if (!root.backend) {
                finish(qsTr("The backend is not ready."), true)
                return
            }
            const gen = ++generation
            busy = true
            failed = false
            status = qsTr("Sending…")
            logos.watch(root.backend.submit({
                action: "token_transfer",
                from: fromField.text,
                to: toField.text,
                amount: amountField.text
            }), function(reply) {
                if (gen !== generation) return
                if (reply.error) {
                    finish(reply.error, true)
                    return
                }
                if (!reply.txId) {
                    finish(qsTr("The engine accepted the transfer but returned no transaction id."), true)
                    return
                }
                txId = reply.txId
                status = qsTr("Sent. Waiting for the block…")
                pollTimer.restart()
            }, function(err) {
                if (gen === generation) finish(String(err), true)
            })
        }

        function poll() {
            const gen = generation
            logos.watch(root.backend.txStatus(txId), function(reply) {
                if (gen !== generation) return
                if (reply.error) {
                    finish(reply.error, true)
                } else if (reply.state === "included") {
                    finish(qsTr("Included in a block."), false)
                } else if (reply.state === "dropped") {
                    finish(qsTr("The network dropped the transfer."), true)
                } else if (reply.state === "next") {
                    status = qsTr("Step %1 of %2 sent.").arg(reply.step + 1).arg(reply.steps)
                }
            }, function(err) {
                if (gen === generation) finish(String(err), true)
            })
        }
    }

    Timer {
        id: pollTimer
        interval: d.pollIntervalMs
        repeat: true
        onTriggered: d.poll()
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Theme.spacing.large
        spacing: Theme.spacing.medium

        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.spacing.xlarge

            LogosText {
                text: qsTr("Token transfer")
                font.pixelSize: Theme.typography.panelTitleText
                font.weight: Theme.typography.weightMedium
                color: Theme.palette.text
            }
            Item { Layout.fillWidth: true }
            LogosBadge {
                text: qsTr("Dev network")
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: Theme.palette.borderSubtle
        }

        LogosText {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            text: qsTr("Fund an account before a take, for example the taker's stable-token holding. "
                       + "Accounts are wallet labels or account ids.")
            color: Theme.palette.textSecondary
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.maximumWidth: 480
            spacing: Theme.spacing.medium

            ColumnLayout {
                Layout.fillWidth: true
                spacing: Theme.spacing.tiny
                LogosText {
                    text: qsTr("From")
                    font.pixelSize: Theme.typography.secondaryText
                    color: Theme.palette.textSecondary
                }
                LogosTextField {
                    id: fromField
                    objectName: "devTransferFrom"
                    Layout.fillWidth: true
                    placeholderText: qsTr("Account label or id")
                    enabled: !d.busy
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: Theme.spacing.tiny
                LogosText {
                    text: qsTr("To")
                    font.pixelSize: Theme.typography.secondaryText
                    color: Theme.palette.textSecondary
                }
                LogosTextField {
                    id: toField
                    objectName: "devTransferTo"
                    Layout.fillWidth: true
                    placeholderText: qsTr("Account label or id")
                    enabled: !d.busy
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: Theme.spacing.tiny
                LogosText {
                    text: qsTr("Amount")
                    font.pixelSize: Theme.typography.secondaryText
                    color: Theme.palette.textSecondary
                }
                LogosTextField {
                    id: amountField
                    objectName: "devTransferAmount"
                    Layout.fillWidth: true
                    placeholderText: qsTr("Whole token units, digits only")
                    enabled: !d.busy
                    textInput.font.features: { "tnum": 1 }
                    validator: RegularExpressionValidator { regularExpression: /^[0-9]*$/ }
                }
                LogosText {
                    objectName: "devTransferAmountEcho"
                    visible: root.amountValid
                    text: Format.amount(amountField.text, 0)
                    font.pixelSize: Theme.typography.secondaryText
                    font.features: { "tnum": 1 }
                    color: Theme.palette.textTertiary
                }
            }

            LogosButton {
                id: submitButton
                objectName: "devTransferSubmit"
                text: qsTr("Transfer")
                variant: LogosButton.Variant.Primary
                enabled: root.canSubmit
                onClicked: d.submit()
                // Primary buttons carry a black label (contrast on the orange fill).
                Component.onCompleted: labelItem.color = Qt.binding(function() {
                    return enabled ? Theme.palette.backgroundBlack : Theme.palette.textMuted
                })
            }

            LogosText {
                id: statusText
                objectName: "devTransferStatus"
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                visible: text.length > 0
                text: d.status
                color: d.failed ? Theme.palette.error : Theme.palette.textSecondary
            }
        }

        Item { Layout.fillHeight: true }
    }
}
