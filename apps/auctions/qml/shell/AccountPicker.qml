import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format

// The active-account picker in the header subtitle (board v5 "Account picker"): a chip with the label,
// short id and a chevron that opens a LogosComboBox-style popup of the wallet's accounts. Each row shows
// label, short id, what the role does, the stable balance and the holding account. The dev keys are left out
// (Dev tools signs with them directly). `rows` is what auction_engine accounts() returns; a failure is its
// only row, `[{error, kind}]`, and shows as a notice, never as an account.
Item {
    id: root
    objectName: "accountPickerRoot"

    property var rows: []
    property string activeLabel: ""

    readonly property alias popup: popup
    readonly property var failure: rows && rows.length > 0 && rows[0] && rows[0].error ? rows[0] : null
    readonly property var accounts: failure ? [] : Format.list(rows).filter(function(r) {
        return r && r.label && root.devKeys.indexOf(r.label) < 0 && root.devKeys.indexOf(r.role) < 0
    })
    readonly property var activeRow: {
        for (let i = 0; i < accounts.length; i++)
            if (accounts[i].label === activeLabel) return accounts[i]
        return null
    }

    // Chosen from the popup; the header assigns it to the backend's activeAccount.
    signal picked(string label)
    // The popup opened: balances may have moved, so the owner re-reads accounts().
    signal refreshRequested()

    // Signed directly by Dev tools; never an account a person acts as.
    readonly property var devKeys: ["engine-admin", "oracle-src"]
    // What each demo role does (board v5). A row whose role is not here shows no role line.
    readonly property var roleTexts: ({
        "owner": qsTr("Opens positions, bids, claims leftover"),
        "keeper": qsTr("Liquidates, earns the reward"),
        "caller": qsTr("Upkeep: settle, check, start auctions"),
        "taker": qsTr("Takes surplus and debt lots"),
        "admin": qsTr("Host and engine admin · Dev tools")
    })

    implicitWidth: chip.implicitWidth
    implicitHeight: chip.implicitHeight

    AbstractButton {
        id: chip
        objectName: "accountPicker"

        implicitHeight: 24
        implicitWidth: chipRow.implicitWidth + leftPadding + rightPadding
        leftPadding: Theme.spacing.small
        rightPadding: Theme.spacing.small
        hoverEnabled: true
        focusPolicy: Qt.StrongFocus
        Accessible.name: qsTr("Active account: %1").arg(root.activeLabel)

        onClicked: popup.opened ? popup.close() : popup.open()

        background: Rectangle {
            color: chip.hovered ? Theme.palette.backgroundMuted : Theme.palette.backgroundSecondary
            border.width: 1
            border.color: popup.visible || chip.activeFocus ? Theme.palette.overlayOrange : Theme.palette.border
            radius: Theme.spacing.radiusSmall
        }

        contentItem: Item {
            implicitWidth: chipRow.implicitWidth
            implicitHeight: chipRow.implicitHeight

            RowLayout {
                id: chipRow
                anchors.verticalCenter: parent.verticalCenter
                spacing: 6

                LogosText {
                    objectName: "headerAccount"
                    text: root.activeLabel.length > 0 ? root.activeLabel : qsTr("No account")
                    font.pixelSize: Theme.typography.primaryText
                    font.weight: Theme.typography.weightMedium
                    color: Theme.palette.text
                }
                LogosText {
                    objectName: "headerAccountId"
                    visible: text.length > 0
                    text: root.activeRow ? Format.shortId(root.activeRow.id) : ""
                    font.family: Theme.typography.mono
                    font.pixelSize: Theme.typography.secondaryText
                    color: Theme.palette.textTertiary
                }
                LogosText {
                    text: popup.visible ? "▲" : "▼"
                    font.pixelSize: 10
                    color: Theme.palette.textTertiary
                }
            }
        }
    }

    Popup {
        id: popup
        objectName: "accountPickerPopup"

        y: chip.height + Theme.spacing.small
        width: 380
        padding: Theme.spacing.tiny
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutsideParent

        onAboutToShow: root.refreshRequested()

        background: Rectangle {
            color: Theme.palette.backgroundSecondary
            border.width: 1
            border.color: Theme.palette.border
            radius: Theme.spacing.radiusLarge
        }

        contentItem: ColumnLayout {
            spacing: 0

            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: Theme.spacing.medium
                Layout.rightMargin: Theme.spacing.medium
                Layout.topMargin: Theme.spacing.small
                Layout.bottomMargin: Theme.spacing.tiny

                LogosText {
                    Layout.fillWidth: true
                    text: qsTr("Sign as")
                    font.pixelSize: Theme.typography.secondaryText
                    color: Theme.palette.textTertiary
                }
                LogosText {
                    text: qsTr("Stable balance")
                    font.pixelSize: Theme.typography.secondaryText
                    color: Theme.palette.textTertiary
                }
            }

            StateNotice {
                objectName: "accountPickerError"
                Layout.fillWidth: true
                Layout.margins: Theme.spacing.small
                reply: root.failure
            }

            Repeater {
                model: root.accounts

                AbstractButton {
                    id: option

                    required property var modelData

                    readonly property bool selected: modelData.label === root.activeLabel
                    readonly property string roleText: root.roleTexts[modelData.role]
                                                       || root.roleTexts[modelData.label] || ""

                    objectName: "accountOption_" + modelData.label
                    Layout.fillWidth: true
                    hoverEnabled: true
                    focusPolicy: Qt.StrongFocus
                    padding: Theme.spacing.small
                    leftPadding: Theme.spacing.medium
                    rightPadding: Theme.spacing.medium
                    Accessible.name: modelData.label

                    onClicked: {
                        root.picked(modelData.label)
                        popup.close()
                    }

                    background: Rectangle {
                        radius: Theme.spacing.radiusLarge
                        color: option.selected || option.hovered || option.activeFocus
                               ? Theme.palette.backgroundButton : "transparent"

                        // The v3 selection bar.
                        Rectangle {
                            visible: option.selected
                            x: 0
                            y: Theme.spacing.small
                            width: 3
                            height: parent.height - 2 * Theme.spacing.small
                            radius: 2
                            color: Theme.palette.accentOrange
                        }
                    }

                    contentItem: GridLayout {
                        columns: 2
                        columnSpacing: Theme.spacing.medium
                        rowSpacing: 2

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 6
                            LogosText {
                                text: option.modelData.label
                                font.pixelSize: Theme.typography.primaryText
                                font.weight: Theme.typography.weightMedium
                                color: Theme.palette.text
                            }
                            LogosText {
                                text: Format.shortId(option.modelData.id)
                                font.family: Theme.typography.mono
                                font.pixelSize: 11
                                color: Theme.palette.textTertiary
                            }
                        }
                        LogosText {
                            objectName: "accountBalance"
                            Layout.alignment: Qt.AlignRight
                            text: option.modelData.stable_balance === null
                                  || option.modelData.stable_balance === undefined
                                  ? "—" : Format.amount(option.modelData.stable_balance, 0)
                            font.pixelSize: Theme.typography.secondaryText
                            font.features: { "tnum": 1 }
                            color: option.selected ? Theme.palette.text : Theme.palette.textSecondary
                        }
                        LogosText {
                            objectName: "accountRole"
                            Layout.fillWidth: true
                            text: option.roleText
                            font.pixelSize: Theme.typography.secondaryText
                            color: Theme.palette.textTertiary
                            elide: Text.ElideRight
                        }
                        LogosText {
                            objectName: "accountHolding"
                            Layout.alignment: Qt.AlignRight
                            text: option.modelData.holding ? option.modelData.holding : "—"
                            font.family: Theme.typography.mono
                            font.pixelSize: 11
                            color: Theme.palette.textTertiary
                        }
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.topMargin: Theme.spacing.tiny
                Layout.preferredHeight: 1
                color: Theme.palette.borderSubtle
            }

            LogosText {
                objectName: "accountPickerNote"
                Layout.fillWidth: true
                Layout.leftMargin: Theme.spacing.medium
                Layout.rightMargin: Theme.spacing.medium
                Layout.topMargin: Theme.spacing.small
                Layout.bottomMargin: Theme.spacing.small
                wrapMode: Text.WordWrap
                text: qsTr("Signing goes through Basecamp's wallet module. Changing account re-reads balances; "
                           + "it never moves funds. The dev keys engine-admin and oracle-src are not listed: "
                           + "Dev tools signs with them directly.")
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textTertiary
            }
        }
    }
}
