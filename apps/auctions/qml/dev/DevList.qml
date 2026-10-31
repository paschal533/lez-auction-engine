import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common/Format.js" as Format

// The left column of Dev tools (board v5): "Dev network" with the network name, then three fixed
// LogosItemDelegate rows (`devItem_<name>`), each a title, a status (`devItemStatus`) and a caption:
//   Oracle price     the price's age (overview.oracle.age_ms), "Move the market price";
//   Token transfer   "Fund a taker account";
//   Protocol token   "Admin" in the error colour (the one destructive action), "Freeze trigger".
ColumnLayout {
    id: root

    property var overview: null
    property string network: ""
    property string selected: ""

    signal picked(string name)

    readonly property var oracle: overview && overview.oracle ? overview.oracle : null

    readonly property var rows: [
        {
            name: "price",
            title: qsTr("Oracle price"),
            status: oracle && oracle.age_ms !== undefined && oracle.age_ms !== null
                    ? qsTr("%1 ago").arg(Format.duration(oracle.age_ms)) : "",
            tone: oracle && oracle.fresh === false ? "error" : "",
            caption: qsTr("Move the market price")
        },
        {
            name: "transfer",
            title: qsTr("Token transfer"),
            status: "",
            tone: "",
            caption: qsTr("Fund a taker account")
        },
        {
            name: "token",
            title: qsTr("Protocol token"),
            status: qsTr("Admin"),
            tone: "error",
            caption: qsTr("Freeze trigger")
        }
    ]

    spacing: Theme.spacing.tiny

    RowLayout {
        Layout.fillWidth: true
        Layout.bottomMargin: Theme.spacing.tiny
        LogosText {
            objectName: "devHeading"
            Layout.fillWidth: true
            text: qsTr("Dev network")
            font.pixelSize: Theme.typography.subtitleText
            color: Theme.palette.text
        }
        LogosText {
            objectName: "devNetworkName"
            text: root.network
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textTertiary
        }
    }

    Repeater {
        model: root.rows

        LogosItemDelegate {
            id: row

            required property var modelData
            readonly property bool isSelected: modelData.name === root.selected

            objectName: "devItem_" + modelData.name
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
                        objectName: "devItemTitle"
                        Layout.fillWidth: true
                        text: row.modelData.title
                        font.pixelSize: Theme.typography.primaryText
                        font.weight: Theme.typography.weightBold
                        color: Theme.palette.text
                        elide: Text.ElideRight
                    }
                    LogosText {
                        objectName: "devItemStatus"
                        visible: text.length > 0
                        text: row.modelData.status
                        font.pixelSize: Theme.typography.secondaryText
                        font.weight: Theme.typography.weightMedium
                        font.features: { "tnum": 1 }
                        color: row.modelData.tone === "error" ? Theme.palette.error : Theme.palette.textSecondary
                    }
                }
                LogosText {
                    objectName: "devItemCaption"
                    Layout.fillWidth: true
                    text: row.modelData.caption
                    font.pixelSize: Theme.typography.secondaryText
                    color: Theme.palette.textTertiary
                    elide: Text.ElideRight
                }
            }
        }
    }

    LogosText {
        objectName: "devSideNote"
        Layout.fillWidth: true
        Layout.topMargin: Theme.spacing.small
        wrapMode: Text.WordWrap
        text: qsTr("Not shown on any other network. Signing uses the dev keys in this deployment's wallet.")
        font.pixelSize: Theme.typography.secondaryText
        color: Theme.palette.textSecondary
    }
}
