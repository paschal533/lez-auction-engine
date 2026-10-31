import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "Format.js" as Format

// Label / value rows of a quote (board v3 `.rows`): 12px tertiary label left, 14px medium value right with
// a 12px unit. Each row is {label, value, unit?, decimals?, strong?}; `decimals` >= 0 formats the value as
// a u128 decimal string, otherwise it is shown as given. Row i's value is `quoteRowValue_<i>`.
ColumnLayout {
    id: root

    property var rows: []
    readonly property int count: repeater.count

    spacing: 0

    Repeater {
        id: repeater
        model: root.rows

        RowLayout {
            id: row

            required property var modelData
            required property int index

            Layout.fillWidth: true
            Layout.minimumHeight: 22
            spacing: Theme.spacing.small

            LogosText {
                Layout.fillWidth: true
                text: row.modelData.label || ""
                font.pixelSize: Theme.typography.secondaryText
                color: row.modelData.strong ? Theme.palette.textSecondary : Theme.palette.textTertiary
                elide: Text.ElideRight
            }
            LogosText {
                objectName: "quoteRowValue_" + row.index
                text: row.modelData.decimals >= 0
                      ? Format.amount(row.modelData.value, row.modelData.decimals)
                      : String(row.modelData.value === undefined ? "" : row.modelData.value)
                font.pixelSize: Theme.typography.primaryText
                font.weight: row.modelData.strong ? Theme.typography.weightBold : Theme.typography.weightMedium
                font.features: { "tnum": 1 }
                color: Theme.palette.text
            }
            LogosText {
                visible: text.length > 0
                text: row.modelData.unit || ""
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textSecondary
            }
        }
    }
}
