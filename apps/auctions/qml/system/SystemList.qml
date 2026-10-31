import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common/Format.js" as Format

// The left column of the System screen (board v5): four fixed LogosItemDelegate rows (`systemItem_<name>`),
// each with a verdict (`systemItemVerdict`) in the palette's verdict colours and a caption
// (`systemItemCaption`), all from the overview:
//   Accounting       Unbacked (error) with the unbacked debt, or Covered (success) with the buffer;
//   Surplus auction  Live (accentOrange) with the lot left, or Idle with the buffer;
//   Debt auction     Live (accentOrange) with the lot left, or Idle with the unbacked debt;
//   Host             Frozen (error) or Live (success), with the freeze threshold (check_solvency may freeze above
//                    it, never at or below).
ColumnLayout {
    id: root

    property var overview: null
    property string selected: ""

    signal picked(string name)

    readonly property var acc: overview && overview.accounting ? overview.accounting : null
    readonly property var host: overview && overview.host ? overview.host : null

    function amount(v) { return Format.amount(String(v), 0) }

    readonly property var rows: [
        {
            name: "accounting",
            title: qsTr("Accounting"),
            verdict: !acc ? "" : Format.cmp(String(acc.unbacked_debt), "0") > 0 ? qsTr("Unbacked") : qsTr("Covered"),
            tone: !acc ? "" : Format.cmp(String(acc.unbacked_debt), "0") > 0 ? "error" : "success",
            caption: !acc ? "" : Format.cmp(String(acc.unbacked_debt), "0") > 0
                     ? qsTr("%1 stable unbacked").arg(amount(acc.unbacked_debt))
                     : qsTr("Buffer %1 stable").arg(amount(acc.buffer))
        },
        {
            name: "surplus",
            title: qsTr("Surplus auction"),
            verdict: !acc ? "" : acc.surplus_live ? qsTr("Live") : qsTr("Idle"),
            tone: acc && acc.surplus_live ? "now" : "",
            caption: !acc ? "" : acc.surplus_live && acc.surplus_lot_left !== null && acc.surplus_lot_left !== undefined
                     ? qsTr("%1 stable left").arg(amount(acc.surplus_lot_left))
                     : qsTr("Buffer %1 stable").arg(amount(acc.buffer))
        },
        {
            name: "debt",
            title: qsTr("Debt auction"),
            verdict: !acc ? "" : acc.debt_live ? qsTr("Live") : qsTr("Idle"),
            tone: acc && acc.debt_live ? "now" : "",
            caption: !acc ? "" : acc.debt_live && acc.debt_lot_left !== null && acc.debt_lot_left !== undefined
                     ? qsTr("%1 stable left").arg(amount(acc.debt_lot_left))
                     : qsTr("Unbacked %1 stable").arg(amount(acc.unbacked_debt))
        },
        {
            name: "host",
            title: qsTr("Host"),
            verdict: !host ? "" : host.frozen ? qsTr("Frozen") : qsTr("Live"),
            tone: !host ? "" : host.frozen ? "error" : "success",
            caption: !acc ? "" : qsTr("Freeze threshold %1").arg(amount(acc.freeze_threshold))
        }
    ]

    spacing: Theme.spacing.tiny

    LogosText {
        objectName: "systemHeading"
        Layout.fillWidth: true
        Layout.bottomMargin: Theme.spacing.tiny
        text: qsTr("Engine")
        font.pixelSize: Theme.typography.subtitleText
        color: Theme.palette.text
    }

    Repeater {
        model: root.rows

        LogosItemDelegate {
            id: row

            required property var modelData
            readonly property bool isSelected: modelData.name === root.selected
            readonly property color verdictColor: modelData.tone === "error" ? Theme.palette.error
                                                  : modelData.tone === "success" ? Theme.palette.success
                                                  : modelData.tone === "now" ? Theme.palette.accentOrange
                                                  : Theme.palette.textSecondary

            objectName: "systemItem_" + modelData.name
            Layout.fillWidth: true
            implicitHeight: content.implicitHeight + 2 * Theme.spacing.medium
            radius: Theme.spacing.radiusLarge
            highlighted: isSelected
            highlightColor: Theme.palette.backgroundButton
            text: ""
            onClicked: root.picked(modelData.name)

            Rectangle {
                visible: row.isSelected
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
                        objectName: "systemItemTitle"
                        Layout.fillWidth: true
                        text: row.modelData.title
                        font.pixelSize: Theme.typography.primaryText
                        font.weight: Theme.typography.weightBold
                        color: Theme.palette.text
                        elide: Text.ElideRight
                    }
                    LogosText {
                        objectName: "systemItemVerdict"
                        text: row.modelData.verdict
                        font.pixelSize: Theme.typography.secondaryText
                        font.weight: Theme.typography.weightMedium
                        color: row.verdictColor
                    }
                }
                LogosText {
                    objectName: "systemItemCaption"
                    Layout.fillWidth: true
                    text: row.modelData.caption
                    font.pixelSize: Theme.typography.secondaryText
                    font.features: { "tnum": 1 }
                    color: Theme.palette.textTertiary
                    elide: Text.ElideRight
                }
            }
        }
    }
}
