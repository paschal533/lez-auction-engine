import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "shell"
import "dev"

Rectangle {
    id: root

    readonly property var backend: logos.module("auctions")

    color: Theme.palette.background

    // The page area of a tab whose content arrives in a later task.
    component EmptyPanel: Rectangle {
        property string heading: ""

        color: Theme.palette.surfaceRaised
        radius: Theme.spacing.radiusXlarge

        LogosText {
            anchors.left: parent.left
            anchors.top: parent.top
            anchors.margins: Theme.spacing.large
            text: parent.heading
            font.pixelSize: Theme.typography.panelTitleText
            font.weight: Theme.typography.weightMedium
            color: Theme.palette.text
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Theme.spacing.xxlarge
        spacing: Theme.spacing.xlarge

        Header {
            id: header
            objectName: "header"
            Layout.fillWidth: true
            network: root.backend ? root.backend.network : ""
            account: root.backend ? root.backend.activeAccount : ""
            reachable: root.backend ? root.backend.nodeReachable : false
            devVisible: root.backend ? root.backend.devNetwork : false
        }

        LogosNotice {
            objectName: "errorNotice"
            Layout.fillWidth: true
            severity: LogosNotice.Error
            message: root.backend ? root.backend.lastError : ""
            shown: message.length > 0
        }

        StackLayout {
            objectName: "pages"
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: header.currentIndex

            EmptyPanel { heading: qsTr("Live auctions") }
            EmptyPanel { heading: qsTr("At-risk positions") }
            EmptyPanel { heading: qsTr("Settled") }
            EmptyPanel { heading: qsTr("System") }
            DevPage {
                objectName: "devPage"
                backend: root.backend
            }
        }
    }
}
