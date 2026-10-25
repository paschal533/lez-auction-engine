import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format

// Confirmation before any transaction (board v4 "Bid confirmation", public dialog). Generic for every
// action: the screen that opens it describes the trade in `request`, next to the request fields it submits:
//   action        the auction_ffi action ("bid", "settle_debt", ...); names the dialog unless `title` is set
//   title, subtitle, confirmLabel, noun ("bid"; "What this <noun> reveals")   optional
//   from          the paying account id (shown short under "You pay")
//   pay, get      {amount, decimals?, unit?, note?}: the "You pay → You get" hero; hidden without both
//   facts         [{label, value, unit?, decimals?, strong?}]: the quote rows (QuoteRows)
//   note          fine print under the trade card
//   reveals       strings for "Visible on-chain"; a generic list by default
// `quote` is the quote reply the request was built from, kept for the caller. Cancel has focus on open and
// Esc cancels (rejected()); Confirm emits confirmed() and closes.
LogosDialog {
    id: root
    objectName: root.scoped("txDialog")

    // Ruling R34: a screen that hosts its own dialog next to another one's prefixes every objectName
    // ("risk" -> riskTxDialog, riskTxConfirm, ...), so the e2e runner finds each by a unique name. Empty
    // keeps the plain names (txDialog, txConfirm, ...).
    property string namePrefix: ""

    function scoped(base) {
        return namePrefix.length > 0 ? namePrefix + base.charAt(0).toUpperCase() + base.substring(1) : base
    }

    property var request: ({})
    property var quote: ({})

    signal confirmed()

    readonly property var titles: ({
        "bid": qsTr("Bid on auction"),
        "liquidate": qsTr("Liquidate position"),
        "claim_leftover": qsTr("Claim leftover"),
        "settle_empty": qsTr("Settle auction"),
        "observe_feed": qsTr("Observe price feed"),
        "settle_debt": qsTr("Settle debt"),
        "check_solvency": qsTr("Check solvency"),
        "start_surplus": qsTr("Start surplus auction"),
        "take_surplus": qsTr("Take surplus lot"),
        "close_surplus": qsTr("Close surplus auction"),
        "start_debt": qsTr("Start debt auction"),
        "take_debt": qsTr("Take debt lot"),
        "close_debt": qsTr("Close debt auction"),
        "token_transfer": qsTr("Transfer tokens"),
        "oracle_publish": qsTr("Publish price"),
        "disable_protocol_token": qsTr("Freeze the engine")
    })

    readonly property var req: request || ({})
    readonly property string titleText: req.title
        ? req.title
        : (titles[req.action] || String(req.action || qsTr("Transaction")).replace(/_/g, " "))
    readonly property bool hasTrade: !!(req.pay && req.get)
    readonly property string noun: req.noun || (req.action === "bid" ? qsTr("bid") : qsTr("transaction"))

    function amountText(part) {
        if (!part) return ""
        return part.decimals >= 0 ? Format.amount(part.amount, part.decimals) : String(part.amount)
    }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: 460
    closePolicy: Popup.CloseOnEscape
    title: titleText

    onOpened: cancelButton.forceActiveFocus()

    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 200; easing.type: Easing.OutCubic }
        NumberAnimation { property: "scale"; from: 0.96; to: 1; duration: 200; easing.type: Easing.OutCubic }
    }

    header: ColumnLayout {
        spacing: 2

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Theme.spacing.large
            Layout.rightMargin: Theme.spacing.large
            Layout.topMargin: Theme.spacing.large
            spacing: Theme.spacing.small

            LogosText {
                objectName: root.scoped("txTitle")
                Layout.fillWidth: true
                text: root.titleText
                font.pixelSize: Theme.typography.primaryText
                font.weight: Theme.typography.weightBold
                color: Theme.palette.text
                elide: Text.ElideRight
            }
            LogosBadge {
                objectName: root.scoped("txMode")
                text: qsTr("Public")
                color: Theme.palette.textSecondary
            }
        }
        LogosText {
            Layout.fillWidth: true
            Layout.leftMargin: Theme.spacing.large
            Layout.rightMargin: Theme.spacing.large
            Layout.bottomMargin: Theme.spacing.small
            visible: text.length > 0
            text: root.req.subtitle || ""
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textTertiary
        }
    }

    contentItem: ColumnLayout {
        spacing: Theme.spacing.medium

        InsetCard {
            Layout.fillWidth: true
            padding: Theme.spacing.large
            spacing: Theme.spacing.small
            visible: root.hasTrade || facts.count > 0

            RowLayout {
                id: swap
                objectName: root.scoped("txSwap")
                Layout.fillWidth: true
                visible: root.hasTrade
                spacing: Theme.spacing.medium

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignBottom
                    spacing: 0
                    LogosText {
                        text: qsTr("You pay")
                        font.pixelSize: Theme.typography.secondaryText
                        color: Theme.palette.textTertiary
                    }
                    RowLayout {
                        spacing: Theme.spacing.tiny
                        LogosText {
                            objectName: root.scoped("txPayAmount")
                            text: root.amountText(root.req.pay)
                            font.pixelSize: Theme.typography.panelTitleText
                            font.weight: Theme.typography.weightBold
                            font.features: { "tnum": 1 }
                            color: Theme.palette.text
                        }
                        LogosText {
                            visible: text.length > 0
                            text: root.req.pay ? (root.req.pay.unit || "") : ""
                            font.pixelSize: Theme.typography.primaryText
                            font.weight: Theme.typography.weightMedium
                            color: Theme.palette.textSecondary
                        }
                    }
                    LogosText {
                        visible: text.length > 0
                        text: root.req.pay && root.req.pay.note ? root.req.pay.note
                              : (root.req.from ? qsTr("from %1").arg(Format.shortId(root.req.from)) : "")
                        font.pixelSize: Theme.typography.secondaryText
                        color: Theme.palette.textSecondary
                    }
                }

                LogosText {
                    Layout.alignment: Qt.AlignVCenter
                    text: "→"
                    font.pixelSize: 18
                    color: Theme.palette.textTertiary
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignBottom
                    spacing: 0
                    LogosText {
                        text: qsTr("You get")
                        font.pixelSize: Theme.typography.secondaryText
                        color: Theme.palette.textTertiary
                    }
                    RowLayout {
                        spacing: Theme.spacing.tiny
                        LogosText {
                            objectName: root.scoped("txGetAmount")
                            text: "≈ " + root.amountText(root.req.get)
                            font.pixelSize: Theme.typography.panelTitleText
                            font.weight: Theme.typography.weightBold
                            font.features: { "tnum": 1 }
                            color: Theme.palette.text
                        }
                        LogosText {
                            visible: text.length > 0
                            text: root.req.get ? (root.req.get.unit || "") : ""
                            font.pixelSize: Theme.typography.primaryText
                            font.weight: Theme.typography.weightMedium
                            color: Theme.palette.textSecondary
                        }
                    }
                    LogosText {
                        visible: text.length > 0
                        text: root.req.get ? (root.req.get.note || "") : ""
                        font.pixelSize: Theme.typography.secondaryText
                        font.features: { "tnum": 1 }
                        color: Theme.palette.textSecondary
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                visible: root.hasTrade && facts.count > 0
                color: Theme.palette.borderSubtle
            }

            QuoteRows {
                id: facts
                objectName: root.scoped("txFacts")
                Layout.fillWidth: true
                rows: root.req.facts || []
            }
        }

        LogosText {
            Layout.fillWidth: true
            visible: text.length > 0
            text: root.req.note || ""
            wrapMode: Text.WordWrap
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textTertiary
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 6

            LogosText {
                text: qsTr("What this %1 reveals").arg(root.noun)
                font.pixelSize: Theme.typography.secondaryText
                font.weight: Theme.typography.weightMedium
                color: Theme.palette.textSecondary
            }

            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: Theme.spacing.small
                uniformCellWidths: true

                InsetCard {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    padding: Theme.spacing.medium
                    spacing: Theme.spacing.tiny

                    LogosText {
                        text: qsTr("Visible on-chain")
                        font.pixelSize: Theme.typography.secondaryText
                        color: Theme.palette.textTertiary
                    }
                    Repeater {
                        model: root.req.reveals || [
                            root.req.from ? qsTr("Your account %1").arg(Format.shortId(root.req.from))
                                          : qsTr("Your account"),
                            qsTr("The amounts in and out"),
                            qsTr("What you did and when")
                        ]
                        LogosText {
                            required property var modelData
                            Layout.fillWidth: true
                            text: modelData
                            wrapMode: Text.WordWrap
                            font.pixelSize: Theme.typography.secondaryText
                            color: Theme.palette.text
                        }
                    }
                }

                InsetCard {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    padding: Theme.spacing.medium
                    spacing: Theme.spacing.tiny

                    LogosText {
                        text: qsTr("Stays private")
                        font.pixelSize: Theme.typography.secondaryText
                        color: Theme.palette.textTertiary
                    }
                    LogosText {
                        objectName: root.scoped("txPrivate")
                        Layout.fillWidth: true
                        text: qsTr("Nothing. Private bids are later work (M4).")
                        wrapMode: Text.WordWrap
                        font.pixelSize: Theme.typography.secondaryText
                        color: Theme.palette.textTertiary
                    }
                    Item { Layout.fillHeight: true }
                }
            }
        }
    }

    rightActions: [
        LogosButton {
            id: cancelButton
            objectName: root.scoped("txCancel")
            text: qsTr("Cancel")
            width: Math.max(96, implicitWidth)
            height: 40
            radius: Theme.spacing.radiusLarge
            onClicked: root.reject()
        },
        LogosButton {
            id: confirmButton
            objectName: root.scoped("txConfirm")
            text: root.req.confirmLabel || qsTr("Confirm")
            width: Math.max(96, implicitWidth)
            height: 40
            radius: Theme.spacing.radiusLarge
            variant: LogosButton.Variant.Primary
            onClicked: {
                root.confirmed()
                root.accept()
            }
            // Primary buttons carry a black label (contrast on the orange fill).
            Component.onCompleted: labelItem.color = Qt.binding(function() {
                return enabled ? Theme.palette.backgroundBlack : Theme.palette.textMuted
            })
        }
    ]
}
