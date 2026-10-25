import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common/Format.js" as Format

// The left column of the Settled screen (board v5, AppManagerView's 200px list): the settled auctions under
// "Scanned to block N · Rescan" (`settledScanned`, `settledRescan`). Each row: short id and the lot, the
// outcome ("x to claim" in accentOrange, "n short" in error, "Claimed", else "Nothing to claim") and how it
// ended ("Target raised" / "Sold out"). Rows keep the view's order, which is the scan cursor's (a set of ids,
// so id order): the chain records no settle time or block (R35), so there is no newest first to show.
ColumnLayout {
    id: root

    // `settled` view items, in view order.
    property var items: []
    property string selectedId: ""
    // The block the scan cursor reached (`scanned_to` of the list), -1 while unknown.
    property int scannedTo: -1
    property bool rescanning: false
    // The last rescan page while one pages, for the progress caption.
    property var lastPage: null

    signal picked(string id)
    signal rescanRequested()

    // What is still claimable: leftover_claimable − leftover_claimed (auction_core claim_leftover).
    function toClaim(it) {
        return Format.subtract(String(it.leftover), String(it.claimed))
    }

    // How it ended: the whole target raised, or the lot sold out first (the two ways a bid ends an auction).
    function endedHow(it) {
        if (Format.cmp(String(it.raised), String(it.target)) >= 0) return qsTr("Target raised")
        if (Format.cmp(String(it.sold), String(it.lot)) >= 0) return qsTr("Sold out")
        return qsTr("Settled")
    }

    spacing: Theme.spacing.small

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 2

        LogosText {
            objectName: "settledHeading"
            Layout.fillWidth: true
            text: qsTr("Settled auctions")
            font.pixelSize: Theme.typography.subtitleText
            color: Theme.palette.text
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.spacing.tiny
            LogosText {
                objectName: "settledScanned"
                Layout.fillWidth: true
                text: root.rescanning
                      ? (root.lastPage ? qsTr("Scanning block %1 of %2").arg(root.lastPage.scanned_to).arg(root.lastPage.head)
                                       : qsTr("Scanning…"))
                      : root.scannedTo >= 0 ? qsTr("Scanned to block %1").arg(root.scannedTo) : ""
                font.pixelSize: Theme.typography.secondaryText
                font.features: { "tnum": 1 }
                color: Theme.palette.textTertiary
                elide: Text.ElideRight
            }
            LogosLink {
                objectName: "settledRescan"
                enabled: !root.rescanning
                text: qsTr("Rescan")
                underline: false
                font.pixelSize: Theme.typography.secondaryText
                onActivated: root.rescanRequested()
            }
        }
    }

    ListView {
        id: list
        objectName: "settledList"
        Layout.fillWidth: true
        Layout.fillHeight: true
        clip: true
        spacing: Theme.spacing.tiny
        model: root.items
        boundsBehavior: Flickable.StopAtBounds

        delegate: LogosItemDelegate {
            id: row

            required property var modelData
            required property int index
            readonly property bool selected: modelData.id === root.selectedId
            readonly property string toClaim: root.toClaim(modelData)
            readonly property bool claimable: Format.cmp(toClaim, "0") > 0
            readonly property bool claimed: Format.cmp(String(modelData.claimed), "0") > 0
            readonly property bool isShort: Format.cmp(String(modelData.shortfall), "0") > 0
            readonly property color accentColor: Theme.palette.accentOrange
            readonly property color errorColor: Theme.palette.error

            objectName: "settledItem_" + modelData.id
            width: ListView.view.width
            height: content.implicitHeight + 2 * Theme.spacing.medium
            radius: Theme.spacing.radiusLarge
            highlighted: selected
            highlightColor: Theme.palette.backgroundButton
            text: ""
            onClicked: root.picked(modelData.id)

            Rectangle {
                visible: row.selected
                x: 0
                anchors.verticalCenter: parent.verticalCenter
                width: 3
                height: parent.height - 2 * Theme.spacing.small
                radius: 1.5
                color: Theme.palette.accentOrange
            }

            contentItem: ColumnLayout {
                id: content
                spacing: 2

                LogosText {
                    Layout.fillWidth: true
                    text: Format.shortId(row.modelData.id)
                    font.pixelSize: Theme.typography.secondaryText
                    font.family: Theme.typography.mono
                    color: Theme.palette.textSecondary
                    elide: Text.ElideRight
                }
                LogosText {
                    objectName: "settledItemLot"
                    Layout.fillWidth: true
                    text: qsTr("%1 collateral").arg(Format.amount(String(row.modelData.lot), 0))
                    font.pixelSize: Theme.typography.subtitleText
                    font.weight: Theme.typography.weightBold
                    font.features: { "tnum": 1 }
                    color: Theme.palette.text
                    elide: Text.ElideRight
                }
                RowLayout {
                    Layout.fillWidth: true
                    LogosText {
                        objectName: "settledItemOutcome"
                        Layout.fillWidth: true
                        text: row.claimable ? qsTr("%1 to claim").arg(Format.amount(row.toClaim, 0))
                              : row.claimed ? qsTr("Claimed")
                              : row.isShort ? qsTr("%1 short").arg(Format.amount(String(row.modelData.shortfall), 0))
                              : qsTr("Nothing to claim")
                        font.pixelSize: Theme.typography.secondaryText
                        font.weight: Theme.typography.weightMedium
                        font.features: { "tnum": 1 }
                        color: row.claimable ? row.accentColor
                               : row.isShort && !row.claimed ? row.errorColor
                               : Theme.palette.textSecondary
                        elide: Text.ElideRight
                    }
                    LogosText {
                        objectName: "settledItemStatus"
                        text: root.endedHow(row.modelData)
                        font.pixelSize: Theme.typography.secondaryText
                        color: Theme.palette.textTertiary
                    }
                }
            }
        }
    }
}
