import QtQuick
import QtTest

import Logos.Theme

import "Fixtures.js" as Fixtures

// The shell header in Main.qml: network dot, node notice, account picker wired to activeAccount.
TestCase {
    id: testCase
    name: "Header"
    width: 1100
    height: 720
    visible: true
    when: windowShown

    property var accountsReply: []
    property int accountsCalls: 0

    function initTestCase() {
        Qt.application.organization = "auctions-qml-tests"
        Qt.application.domain = "auctions.test"
    }

    QtObject {
        id: logos

        property var backendStub: QtObject {
            property string network: "localnet"
            property bool devNetwork: true
            property bool walletOpen: true
            property string activeAccount: "owner"
            property bool nodeReachable: true
            property int blockHeight: 3
            property string lastError: ""
            function accounts() { testCase.accountsCalls++; return { call: "accounts" } }
            function submit(req) { return { call: "submit" } }
            function txStatus(id) { return { call: "txStatus" } }
            function view(req) { return { call: "view" } }
        }

        function module(name) { return backendStub }
        function watch(promise, ok, err) {
            if (promise && promise.call === "accounts")
                ok(testCase.accountsReply)
        }
    }

    function init() {
        accountsReply = Fixtures.load("accounts")
        accountsCalls = 0
        logos.backendStub.activeAccount = "owner"
        logos.backendStub.nodeReachable = true
    }

    function createMain() {
        const component = Qt.createComponent("../../qml/Main.qml")
        compare(component.status, Component.Ready, component.errorString())
        const item = createTemporaryObject(component, testCase, { width: 1100, height: 720 })
        verify(item !== null)
        return item
    }

    // Review Focus 1: the node stops answering mid-session. The header says so and the app stays usable.
    function test_header_shows_node_unreachable() {
        const main = createMain()
        const notice = findChild(main, "nodeNotice")
        verify(!notice.visible, "no notice while the node answers")

        logos.backendStub.nodeReachable = false
        verify(notice.visible)
        compare(notice.title, "Node not answering")
        compare(findChild(main, "headerNetworkDot").color, Theme.palette.error)
        // Still responsive: tabs switch and the picker opens.
        mouseClick(findChild(main, "tabSettled"))
        compare(findChild(main, "pages").currentIndex, 2)
        findChild(main, "accountPicker").clicked()
        tryCompare(findChild(main, "accountPickerRoot").popup, "opened", true)

        logos.backendStub.nodeReachable = true
        verify(!notice.visible)
    }

    function test_picking_an_account_updates_the_header() {
        const main = createMain()
        compare(findChild(main, "headerAccount").text, "owner")
        const picker = findChild(main, "accountPickerRoot")
        findChild(main, "accountPicker").clicked()
        tryCompare(picker.popup, "opened", true)
        findChild(picker.popup.contentItem, "accountOption_keeper").clicked()
        // R14: activeAccount is a READWRITE prop; the view assigns it.
        compare(logos.backendStub.activeAccount, "keeper")
        compare(findChild(main, "headerAccount").text, "keeper")
        compare(findChild(main, "headerAccountId").text, "5HSa…KKeE")
    }

    function test_accounts_are_read_again_when_the_account_changes() {
        const main = createMain()
        const before = accountsCalls
        verify(before >= 1)
        logos.backendStub.activeAccount = "taker"
        verify(accountsCalls > before, "balances re-read after an account switch")
    }

    function test_accounts_failure_shows_a_notice_not_an_account() {
        accountsReply = [{ error: "the node did not answer", kind: "chain" }]
        const main = createMain()
        const notice = findChild(main, "accountsNotice")
        verify(notice.visible)
        verify(notice.message.indexOf("the node did not answer") >= 0, notice.message)
        compare(findChild(main, "headerAccount").text, "owner")
        compare(findChild(main, "headerAccountId").text, "")
    }
}
