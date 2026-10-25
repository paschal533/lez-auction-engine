import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

// Dev tools: the left list of actions and the detail panel, same grid as Applications.
// Token transfer is the first action; the others arrive with the demo tasks.
RowLayout {
    id: root

    property var backend: null

    spacing: Theme.spacing.xlarge

    ColumnLayout {
        Layout.preferredWidth: 200
        Layout.minimumWidth: 160
        Layout.maximumWidth: 200
        Layout.fillHeight: true
        spacing: Theme.spacing.tiny

        LogosText {
            Layout.topMargin: Theme.spacing.tiny
            Layout.bottomMargin: Theme.spacing.tiny
            text: qsTr("Dev network")
            font.pixelSize: Theme.typography.subtitleText
            color: Theme.palette.text
        }

        LogosItemDelegate {
            objectName: "devItemTransfer"
            Layout.fillWidth: true
            text: qsTr("Token transfer")
            highlighted: true
            radius: Theme.spacing.radiusLarge
            highlightColor: Theme.palette.backgroundButton
            hoverColor: "transparent"
            textColor: Theme.palette.text
        }

        Item { Layout.fillHeight: true }
    }

    TransferDetail {
        Layout.fillWidth: true
        Layout.fillHeight: true
        backend: root.backend
    }
}
