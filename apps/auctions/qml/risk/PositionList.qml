import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common/Format.js" as Format

// The left column of the At-risk screen (board v4, AppManagerView's 200px list): scanned positions with debt,
// lowest CR first, under "Scanned to block N · Rescan" (`riskScanned`, `riskRescan`). Each row: short id and a
// Liquidatable/Healthy badge, "CR x.x%" (floored like the CLI, R32) with the minimum, and collateral · debt.
// CR is the host's min-CR ratio at the redemption price (R17). The selected row carries the LogosItemDelegate
// highlight and the 3px accentOrange bar.
ColumnLayout {
    id: root

    // `at_risk` view items, any order.
    property var items: []
    property string selectedId: ""
    // The block the scan cursor reached (`scanned_to` of the list), -1 while unknown.
    property int scannedTo: -1
    property bool rescanning: false
    // The last rescan page while one pages, for the progress caption.
    property var lastPage: null
    readonly property var sorted: sortItems(items)

    signal picked(string id)
    signal rescanRequested()

    // Lowest CR first (the view's order, kept here so the screen never depends on it); ties keep view order.
    function sortItems(list) {
        const copy = (list || []).map(function(it, i) { return { it: it, i: i } })
        copy.sort(function(a, b) {
            const byCr = Number(a.it.cr_bps) - Number(b.it.cr_bps)
            return byCr !== 0 ? byCr : a.i - b.i
        })
        return copy.map(function(x) { return x.it })
    }

    spacing: Theme.spacing.small

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 2

        LogosText {
            objectName: "riskHeading"
            Layout.fillWidth: true
            text: qsTr("Lowest CR first")
            font.pixelSize: Theme.typography.subtitleText
            color: Theme.palette.text
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.spacing.tiny
            LogosText {
                objectName: "riskScanned"
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
                objectName: "riskRescan"
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
        objectName: "riskList"
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
            readonly property bool selected: modelData.position === root.selectedId
            readonly property bool liquidatable: !!modelData.liquidatable

            objectName: "riskItem_" + modelData.position
            width: ListView.view.width
            height: content.implicitHeight + 2 * Theme.spacing.medium
            radius: Theme.spacing.radiusLarge
            highlighted: selected
            highlightColor: Theme.palette.backgroundButton
            text: ""
            onClicked: root.picked(modelData.position)

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
                        text: Format.shortId(row.modelData.position)
                        font.pixelSize: Theme.typography.secondaryText
                        font.family: Theme.typography.mono
                        color: Theme.palette.textSecondary
                        elide: Text.ElideRight
                    }
                    LogosBadge {
                        objectName: "riskItemBadge"
                        text: row.liquidatable ? qsTr("Liquidatable") : qsTr("Healthy")
                        color: row.liquidatable ? Theme.palette.error : Theme.palette.textSecondary
                    }
                }
                RowLayout {
                    Layout.fillWidth: true
                    LogosText {
                        objectName: "riskItemCr"
                        text: qsTr("CR %1%").arg(Format.bpsPercent(row.modelData.cr_bps, 1))
                        font.pixelSize: Theme.typography.subtitleText
                        font.weight: Theme.typography.weightBold
                        font.features: { "tnum": 1 }
                        color: row.liquidatable ? Theme.palette.error : Theme.palette.text
                    }
                    Item { Layout.fillWidth: true }
                    LogosText {
                        objectName: "riskItemMin"
                        text: qsTr("min %1%").arg(Format.bpsPercentTrim(row.modelData.min_cr_bps))
                        font.pixelSize: Theme.typography.secondaryText
                        font.features: { "tnum": 1 }
                        color: Theme.palette.textSecondary
                    }
                }
                LogosText {
                    objectName: "riskItemAmounts"
                    Layout.fillWidth: true
                    text: qsTr("%1 collateral · %2 stable").arg(Format.amount(String(row.modelData.collateral), 0))
                                                          .arg(Format.amount(String(row.modelData.nominal_debt), 0))
                    font.pixelSize: Theme.typography.secondaryText
                    font.features: { "tnum": 1 }
                    color: Theme.palette.textSecondary
                    wrapMode: Text.WordWrap
                }
            }
        }
    }
}
