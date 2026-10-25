import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common/Format.js" as Format

// The collateralisation gauge of the At-risk screen (board v4): a 100–250 % track, an error-tinted zone below
// the liquidation threshold, a white tick at the threshold (`min_cr_bps`) and a marker at the position's CR
// (`cr_bps`). Both come from the `position` view; CR is the host's min-CR ratio at the redemption price (R17).
// The marker clamps to the track ends; without debt (`crBps` < 0) it is hidden.
ColumnLayout {
    id: root
    objectName: "crGauge"

    // Basis points (u64 in the view, so real: an int would wrap); -1 when the view has no CR (no debt).
    property real crBps: -1
    property real minCrBps: 0
    property bool liquidatable: false

    readonly property int axisLowBps: 10000
    readonly property int axisHighBps: 25000

    function ratioOf(bps) {
        return Math.max(0, Math.min(1, (bps - axisLowBps) / (axisHighBps - axisLowBps)))
    }

    readonly property real markerRatio: crBps >= 0 ? ratioOf(crBps) : 0
    readonly property real tickRatio: ratioOf(minCrBps)

    spacing: Theme.spacing.small

    Item {
        id: track
        Layout.fillWidth: true
        Layout.preferredHeight: 18

        Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width
            height: 6
            radius: 3
            color: Theme.palette.backgroundButton
        }
        // Below the threshold the host lets the position be liquidated.
        Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width * root.tickRatio
            height: 6
            radius: 3
            color: Theme.palette.error
            opacity: 0.35
        }
        Rectangle {
            objectName: "crTick"
            x: parent.width * root.tickRatio - width / 2
            anchors.verticalCenter: parent.verticalCenter
            width: 2
            height: parent.height
            color: Theme.palette.text
        }
        Rectangle {
            objectName: "crMarker"
            visible: root.crBps >= 0
            x: Math.max(0, Math.min(parent.width - width, parent.width * root.markerRatio - width / 2))
            anchors.verticalCenter: parent.verticalCenter
            width: 10
            height: 16
            radius: 3
            color: root.liquidatable ? Theme.palette.error : Theme.palette.warning
            border.color: Theme.palette.background
            border.width: 1
        }
    }

    Item {
        Layout.fillWidth: true
        Layout.preferredHeight: lowLabel.implicitHeight

        LogosText {
            id: lowLabel
            text: Format.bpsPercent(root.axisLowBps, 0) + "%"
            font.pixelSize: Theme.typography.secondaryText
            font.features: { "tnum": 1 }
            color: Theme.palette.textTertiary
        }
        LogosText {
            objectName: "crTickLabel"
            x: Math.max(lowLabel.implicitWidth + Theme.spacing.small,
                        Math.min(parent.width - implicitWidth - highLabel.implicitWidth - Theme.spacing.small,
                                 parent.width * root.tickRatio - implicitWidth / 2))
            text: qsTr("%1% liquidation threshold").arg(Format.bpsPercentTrim(root.minCrBps))
            font.pixelSize: Theme.typography.secondaryText
            font.weight: Theme.typography.weightMedium
            font.features: { "tnum": 1 }
            color: Theme.palette.text
        }
        LogosText {
            id: highLabel
            anchors.right: parent.right
            text: Format.bpsPercent(root.axisHighBps, 0) + "%"
            font.pixelSize: Theme.typography.secondaryText
            font.features: { "tnum": 1 }
            color: Theme.palette.textTertiary
        }
    }
}
