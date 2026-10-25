import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format

// "Where the N went" (board v5): one stacked bar and its legend, from three fields of the `settled` view as
// they are, never derived (auction_core/src/auction.rs):
//   debt repaid (debt_repaid, textTertiary) and penalty to the buffer (penalty, accentOrange) are what bidders
//   paid, so together they are `raised` (N);
//   shortfall (error, faded) is the debt that was not repaid (debt_principal − debt_raised), booked as unbacked
//   debt. It was never raised, so it sits after the raised part instead of inside it. A penalty that was not
//   raised is not booked anywhere and is not drawn.
// Each segment's width is its share of debt repaid + penalty + shortfall. Segment items `outcomeDebt`,
// `outcomePenalty`, `outcomeShortfall` carry their field in `value`.
InsetCard {
    id: root
    objectName: "outcomeBar"

    property string debtRepaid: "0"
    property string penalty: "0"
    property string shortfall: "0"
    property string raised: "0"

    readonly property string whole: Format.add(Format.add(debtRepaid, penalty), shortfall)
    readonly property bool nothingRaised: Format.cmp(raised, "0") === 0

    padding: Theme.spacing.large
    spacing: Theme.spacing.medium

    // A coloured swatch, the label and the amount (`legend*` text: "Label amount").
    component LegendItem: RowLayout {
        id: item
        property color swatch: Theme.palette.textTertiary
        property string label: ""
        property string amount: "0"
        readonly property alias text: legendText.text

        spacing: Theme.spacing.tiny
        Rectangle {
            Layout.preferredWidth: 8
            Layout.preferredHeight: 8
            radius: 2
            color: item.swatch
        }
        LogosText {
            id: legendText
            text: item.label + " " + Format.amount(item.amount, 0)
            font.pixelSize: Theme.typography.secondaryText
            font.features: { "tnum": 1 }
            color: Theme.palette.textSecondary
        }
    }

    component Segment: Rectangle {
        property string value: "0"
        height: parent.height
        width: root.whole.length > 0 && Format.cmp(root.whole, "0") > 0
               ? parent.width * Format.ratio(value, root.whole) : 0
    }

    RowLayout {
        Layout.fillWidth: true
        LogosText {
            objectName: "outcomeHeading"
            Layout.fillWidth: true
            text: root.nothingRaised ? qsTr("Nothing was raised")
                                     : qsTr("Where the %1 stable went").arg(Format.amount(root.raised, 0))
            font.pixelSize: Theme.typography.primaryText
            font.weight: Theme.typography.weightBold
            font.features: { "tnum": 1 }
            color: Theme.palette.text
        }
    }

    Rectangle {
        id: track
        objectName: "outcomeTrack"
        Layout.fillWidth: true
        Layout.preferredHeight: 10
        radius: 3
        color: Theme.palette.backgroundButton
        clip: true

        Row {
            anchors.fill: parent
            Segment {
                objectName: "outcomeDebt"
                value: root.debtRepaid
                color: Theme.palette.textTertiary
            }
            Segment {
                objectName: "outcomePenalty"
                value: root.penalty
                color: Theme.palette.accentOrange
            }
            Segment {
                objectName: "outcomeShortfall"
                value: root.shortfall
                color: Qt.rgba(Theme.palette.error.r, Theme.palette.error.g, Theme.palette.error.b, 0.45)
            }
        }
    }

    Flow {
        Layout.fillWidth: true
        spacing: Theme.spacing.large
        LegendItem {
            objectName: "legendDebt"
            swatch: Theme.palette.textTertiary
            label: qsTr("Debt repaid")
            amount: root.debtRepaid
        }
        LegendItem {
            objectName: "legendPenalty"
            swatch: Theme.palette.accentOrange
            label: qsTr("Penalty to the buffer")
            amount: root.penalty
        }
        LegendItem {
            objectName: "legendShortfall"
            swatch: Qt.rgba(Theme.palette.error.r, Theme.palette.error.g, Theme.palette.error.b, 0.45)
            label: qsTr("Shortfall")
            amount: root.shortfall
        }
    }

    LogosText {
        objectName: "outcomeNote"
        Layout.fillWidth: true
        wrapMode: Text.WordWrap
        text: qsTr("Debt repaid and penalty are what bidders paid. The shortfall is debt that was not repaid; it became unbacked debt.")
        font.pixelSize: Theme.typography.secondaryText
        color: Theme.palette.textTertiary
    }
}
