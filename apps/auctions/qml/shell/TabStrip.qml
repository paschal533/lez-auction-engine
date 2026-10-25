import QtQuick
import QtQuick.Controls

import Logos.Theme
import Logos.Controls

// The five view tabs, in the header's right slot. Dev tools shows only on a dev network.
LogosTabBar {
    id: root

    property bool devVisible: false

    spacing: Theme.spacing.large

    // A hidden current tab would leave the stack on a page nobody can reach.
    onDevVisibleChanged: if (!devVisible && currentIndex === 4) currentIndex = 0

    LogosTabButton {
        objectName: "tabLive"
        text: qsTr("Live auctions")
    }
    LogosTabButton {
        objectName: "tabRisk"
        text: qsTr("At-risk positions")
    }
    LogosTabButton {
        objectName: "tabSettled"
        text: qsTr("Settled")
    }
    LogosTabButton {
        objectName: "tabSystem"
        text: qsTr("System")
    }
    LogosTabButton {
        objectName: "tabDev"
        text: qsTr("Dev tools")
        visible: root.devVisible
    }
}
