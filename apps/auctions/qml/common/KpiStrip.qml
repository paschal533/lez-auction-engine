import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "Format.js" as Format

// A row of KPIs in an inset card (board v3 `.kpis`): each cell a 12px tertiary label, a 24px bold value
// (accentOrange when it is the "now" figure) with a unit, and a 12px caption; cells split by borderSubtle
// hairlines. Each item is {label, value, unit?, decimals?, caption?, accent?, progress?}; `decimals` >= 0 formats
// the value as a u128 decimal string; `progress` (0–1) adds an accentOrange bar under the value (raised of
// target). Cell i's label, value and caption are `kpiLabel_<i>`, `kpiValue_<i>`, `kpiCaption_<i>`.
InsetCard {
    id: root

    property var items: []

    padding: 0

    RowLayout {
        Layout.fillWidth: true
        Layout.topMargin: Theme.spacing.medium
        Layout.bottomMargin: Theme.spacing.medium
        spacing: 0

        Repeater {
            model: root.items

            RowLayout {
                id: cell

                required property var modelData
                required property int index

                Layout.fillWidth: true
                spacing: 0

                Rectangle {
                    visible: cell.index > 0
                    Layout.fillHeight: true
                    Layout.preferredWidth: 1
                    color: Theme.palette.borderSubtle
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.leftMargin: Theme.spacing.large
                    Layout.rightMargin: Theme.spacing.large
                    spacing: 0

                    LogosText {
                        objectName: "kpiLabel_" + cell.index
                        text: cell.modelData.label || ""
                        font.pixelSize: Theme.typography.secondaryText
                        color: Theme.palette.textTertiary
                    }
                    RowLayout {
                        spacing: Theme.spacing.tiny
                        LogosText {
                            objectName: "kpiValue_" + cell.index
                            text: cell.modelData.decimals >= 0
                                  ? Format.amount(cell.modelData.value, cell.modelData.decimals)
                                  : String(cell.modelData.value === undefined ? "" : cell.modelData.value)
                            font.pixelSize: Theme.typography.panelTitleText
                            font.weight: Theme.typography.weightBold
                            font.features: { "tnum": 1 }
                            color: cell.modelData.accent ? Theme.palette.accentOrange : Theme.palette.text
                        }
                        LogosText {
                            visible: text.length > 0
                            text: cell.modelData.unit || ""
                            font.pixelSize: Theme.typography.primaryText
                            font.weight: Theme.typography.weightMedium
                            color: Theme.palette.textSecondary
                        }
                    }
                    LogosProgressBar {
                        objectName: "kpiProgress_" + cell.index
                        visible: cell.modelData.progress !== undefined
                        Layout.fillWidth: true
                        Layout.topMargin: Theme.spacing.tiny
                        Layout.preferredHeight: 4
                        from: 0
                        to: 1
                        value: cell.modelData.progress || 0
                        fillColor: Theme.palette.accentOrange
                    }
                    LogosText {
                        objectName: "kpiCaption_" + cell.index
                        visible: text.length > 0
                        text: cell.modelData.caption || ""
                        font.pixelSize: Theme.typography.secondaryText
                        font.features: { "tnum": 1 }
                        color: Theme.palette.textSecondary
                    }
                }
            }
        }
    }
}
