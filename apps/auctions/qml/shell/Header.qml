import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

// Page header, same grid as Applications and Settings: title and subtitle on the left,
// the view tabs in the right slot. The subtitle ends with the active-account picker (board v5).
RowLayout {
    id: root

    property string network: ""
    property string account: ""
    // What auction_engine accounts() returned: the picker rows, or [{error, kind}].
    property var accounts: []
    property bool reachable: false
    property alias currentIndex: tabs.currentIndex
    property alias devVisible: tabs.devVisible

    signal accountPicked(string label)
    signal accountsRefreshRequested()

    readonly property string networkLabel: network.length > 0
        ? network.charAt(0).toUpperCase() + network.slice(1)
        : qsTr("No network")

    spacing: Theme.spacing.xlarge

    ColumnLayout {
        spacing: Theme.spacing.tiny

        LogosText {
            objectName: "headerTitle"
            text: qsTr("Auctions")
            font.pixelSize: Theme.typography.pageTitleText
            font.weight: Theme.typography.weightBold
            color: Theme.palette.text
        }

        RowLayout {
            objectName: "headerSubtitle"
            spacing: Theme.spacing.small

            LogosText {
                text: qsTr("Liquidation auctions") + " ·"
                color: Theme.palette.textSecondary
            }
            Rectangle {
                objectName: "headerNetworkDot"
                Layout.preferredWidth: 6
                Layout.preferredHeight: 6
                radius: 3
                color: root.reachable ? Theme.palette.success : Theme.palette.error
            }
            LogosText {
                objectName: "headerNetwork"
                text: root.networkLabel + " ·"
                color: Theme.palette.textSecondary
            }
            AccountPicker {
                rows: root.accounts
                activeLabel: root.account
                onPicked: function(label) { root.accountPicked(label) }
                onRefreshRequested: root.accountsRefreshRequested()
            }
        }
    }

    Item { Layout.fillWidth: true }

    TabStrip {
        id: tabs
        Layout.alignment: Qt.AlignRight | Qt.AlignTop
        Layout.preferredWidth: implicitWidth
    }
}
