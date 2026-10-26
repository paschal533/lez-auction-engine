import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common/Format.js" as Format

// The left column of the Live screen (board v3, AppManagerView's 200px list): live auctions, deepest discount
// first (R30: the engine has no end time, so there is no "ending soonest"). Each row: short id, the time the
// maximum is reached (or "Held at max"), the lot, the current discount in accentOrange and a raised-of-target
// bar. The selected row carries the LogosItemDelegate highlight and the 3px accentOrange bar.
ColumnLayout {
    id: root

    // `live` view items, any order.
    property var items: []
    property string selectedId: ""
    readonly property var sorted: sortItems(items)

    signal picked(string id)

    // Deepest discount first (discount_ray descending); ties: the one reaching its maximum earlier first.
    function sortItems(list) {
        const copy = (list || []).slice()
        copy.sort(function(a, b) {
            const byDiscount = Format.cmp(String(b.discount_ray), String(a.discount_ray))
            if (byDiscount !== 0) return byDiscount
            return Number(a.max_at_ms) - Number(b.max_at_ms)
        })
        return copy
    }

    spacing: Theme.spacing.small

    RowLayout {
        Layout.fillWidth: true
        LogosText {
            objectName: "liveHeading"
            Layout.fillWidth: true
            text: qsTr("Deepest discount first")
            font.pixelSize: Theme.typography.subtitleText
            color: Theme.palette.text
        }
        LogosText {
            objectName: "liveCount"
            text: qsTr("%1 live").arg(root.sorted.length)
            font.pixelSize: Theme.typography.secondaryText
            font.features: { "tnum": 1 }
            color: Theme.palette.textTertiary
        }
    }

    ListView {
        id: list
        objectName: "liveList"
        Layout.fillWidth: true
        Layout.fillHeight: true
        clip: true
        spacing: Theme.spacing.tiny
        model: root.sorted
        boundsBehavior: Flickable.StopAtBounds

        delegate: LogosItemDelegate {
            id: row

            required property var modelData
            required property int index
            readonly property bool selected: modelData.id === root.selectedId

            objectName: "liveItem_" + modelData.id
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

                RowLayout {
                    Layout.fillWidth: true
                    LogosText {
                        Layout.fillWidth: true
                        text: Format.shortId(row.modelData.id)
                        font.pixelSize: Theme.typography.secondaryText
                        font.family: Theme.typography.mono
                        color: Theme.palette.textSecondary
                        elide: Text.ElideRight
                    }
                    // Board v4 "Live auction, paused": a paused feed stops the discount clock, so there is no
                    // max time to show.
                    LogosText {
                        objectName: "liveItemWhen"
                        text: row.modelData.paused
                              ? qsTr("Paused")
                              : row.modelData.held_at_max
                              ? qsTr("Held at max")
                              : qsTr("Max at %1").arg(Qt.formatTime(new Date(Number(row.modelData.max_at_ms)), "hh:mm"))
                        font.pixelSize: Theme.typography.secondaryText
                        font.features: { "tnum": 1 }
                        color: row.modelData.paused ? Theme.palette.warning : Theme.palette.textSecondary
                    }
                }
                RowLayout {
                    Layout.fillWidth: true
                    LogosText {
                        objectName: "liveItemLot"
                        text: Format.amount(row.modelData.lot, 0)
                        font.pixelSize: Theme.typography.subtitleText
                        font.weight: Theme.typography.weightMedium
                        font.features: { "tnum": 1 }
                        color: Theme.palette.text
                    }
                    Item { Layout.fillWidth: true }
                    LogosText {
                        objectName: "liveItemDiscount"
                        text: Format.rayPercent(row.modelData.discount_ray, 1) + "%"
                        font.pixelSize: Theme.typography.subtitleText
                        font.weight: Theme.typography.weightMedium
                        font.features: { "tnum": 1 }
                        // Greyed while paused: the discount is frozen, not "now".
                        color: row.modelData.paused ? Theme.palette.textTertiary : Theme.palette.accentOrange
                    }
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.spacing.small
                    LogosProgressBar {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 4
                        from: 0
                        to: 1
                        value: Format.ratio(row.modelData.raised, row.modelData.target)
                        fillColor: Theme.palette.accentOrange
                    }
                    LogosText {
                        objectName: "liveItemRaised"
                        text: qsTr("%1% raised").arg(Format.percentFloor(row.modelData.raised, row.modelData.target))
                        font.pixelSize: Theme.typography.secondaryText
                        font.features: { "tnum": 1 }
                        color: Theme.palette.textSecondary
                    }
                }
            }
        }
    }
}
