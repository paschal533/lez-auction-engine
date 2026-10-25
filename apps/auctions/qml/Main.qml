import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "common"
import "shell"
import "dev"
import "live"
import "risk"

Rectangle {
    id: root

    readonly property var backend: logos.module("auctions")

    color: Theme.palette.background

    QtObject {
        id: d

        // What accounts() last returned: picker rows, or [{error, kind}] (the module's in-band failure).
        property var accounts: []
        // A reply for a request older than the latest is dropped.
        property int accountsGeneration: 0

        function refreshAccounts() {
            if (!root.backend)
                return
            const gen = ++accountsGeneration
            logos.watch(root.backend.accounts(), function(rows) {
                if (gen !== accountsGeneration) return
                accounts = Array.isArray(rows)
                    ? rows
                    : [{ error: qsTr("The engine sent an unreadable account list."), kind: "bad_reply" }]
            }, function(err) {
                if (gen === accountsGeneration) accounts = [{ error: String(err), kind: "ipc" }]
            })
        }
    }

    Component.onCompleted: d.refreshAccounts()

    // Changing account re-reads balances (board v5 picker note).
    Connections {
        target: root.backend
        ignoreUnknownSignals: true
        function onActiveAccountChanged() { d.refreshAccounts() }
        function onWalletOpenChanged() { d.refreshAccounts() }
    }

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
            accounts: d.accounts
            reachable: root.backend ? root.backend.nodeReachable : false
            devVisible: root.backend ? root.backend.devNetwork : false
            // R14: activeAccount is a READWRITE prop; assigning it reaches the backend.
            onAccountPicked: function(label) { if (root.backend) root.backend.activeAccount = label }
            onAccountsRefreshRequested: d.refreshAccounts()
        }

        NodeNotice {
            Layout.fillWidth: true
            active: root.backend ? !!root.backend.walletOpen : false
            reachable: root.backend ? root.backend.nodeReachable : true
        }

        StateNotice {
            objectName: "accountsNotice"
            Layout.fillWidth: true
            prefix: qsTr("Accounts")
            reply: d.accounts.length > 0 && d.accounts[0] && d.accounts[0].error ? d.accounts[0] : null
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

            LivePage {
                backend: root.backend
                accounts: d.accounts
                activeAccount: root.backend ? root.backend.activeAccount : ""
            }
            RiskPage {
                backend: root.backend
                accounts: d.accounts
                activeAccount: root.backend ? root.backend.activeAccount : ""
            }
            EmptyPanel { heading: qsTr("Settled") }
            EmptyPanel { heading: qsTr("System") }
            DevPage {
                objectName: "devPage"
                backend: root.backend
            }
        }
    }
}
