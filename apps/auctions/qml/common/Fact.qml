import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "Format.js" as Format

// One labelled figure of a facts grid (board v3 `.fact`): a 12px tertiary label over a 14px medium value
// with an optional unit. `decimals` >= 0 formats `value` as a u128 decimal string (Format.amount);
// otherwise `value` is shown as given.
ColumnLayout {
    id: root

    property string label: ""
    property string value: ""
    property string unit: ""
    property int decimals: -1
    property color valueColor: Theme.palette.text

    readonly property string displayValue: decimals >= 0 ? Format.amount(value, decimals) : value

    spacing: 2

    LogosText {
        objectName: "factLabel"
        text: root.label
        font.pixelSize: Theme.typography.secondaryText
        color: Theme.palette.textTertiary
    }

    RowLayout {
        spacing: Theme.spacing.tiny

        LogosText {
            objectName: "factValue"
            text: root.displayValue
            font.pixelSize: Theme.typography.primaryText
            font.weight: Theme.typography.weightMedium
            font.features: { "tnum": 1 }
            color: root.valueColor
        }
        LogosText {
            visible: root.unit.length > 0
            text: root.unit
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textSecondary
        }
    }
}
