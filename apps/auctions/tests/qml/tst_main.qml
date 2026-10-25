import QtQuick
import QtTest

// Loads Main.qml against the real design system with a stub `logos` object and checks the shell.
TestCase {
    id: testCase
    name: "Main"
    width: 1100
    height: 720
    visible: true
    when: windowShown

    property var calls: []

    // The runtime object Basecamp provides; here a stub backend whose slots record their requests.
    QtObject {
        id: logos

        property var backendStub: QtObject {
            property string network: "localnet"
            property bool devNetwork: true
            property string activeAccount: "admin"
            property bool nodeReachable: true
            property string lastError: ""
            function submit(req) { testCase.calls.push(req); return req }
            function txStatus(id) { return id }
        }

        function module(name) { return backendStub }
        // The stub runs the success callback at once, with a canned reply per request.
        function watch(promise, ok, err) {
            if (promise && promise.action === "token_transfer")
                ok({ txId: "tx1" })
            else
                ok({ state: "included" })
        }
    }

    function createMain() {
        const component = Qt.createComponent("../../qml/Main.qml")
        compare(component.status, Component.Ready, component.errorString())
        const item = component.createObject(testCase, { width: 1100, height: 720 })
        verify(item !== null)
        return item
    }

    function test_header_and_tabs() {
        const main = createMain()
        compare(findChild(main, "headerTitle").text, "Auctions")
        compare(findChild(main, "headerNetwork").text, "Localnet ·")
        compare(findChild(main, "headerAccount").text, "admin")
        const names = ["tabLive", "tabRisk", "tabSettled", "tabSystem", "tabDev"]
        for (let i = 0; i < names.length; i++)
            verify(findChild(main, names[i]) !== null, names[i])
        verify(findChild(main, "tabDev").visible)
        main.destroy()
    }

    function test_dev_tab_hidden_off_dev_network() {
        logos.backendStub.devNetwork = false
        const main = createMain()
        verify(!findChild(main, "tabDev").visible)
        // The hidden tab takes no room in the strip.
        const right = findChild(main, "tabSystem").mapToItem(main, 0, 0).x + findChild(main, "tabSystem").width
        const bar = findChild(main, "tabSystem").parent.parent
        verify(bar.implicitWidth <= right - bar.mapToItem(main, 0, 0).x + 1, "bar " + bar.implicitWidth + " right " + right)
        main.destroy()
        logos.backendStub.devNetwork = true
    }

    function test_transfer_form_submits_a_token_transfer() {
        const main = createMain()
        const submit = findChild(main, "devTransferSubmit")
        verify(!submit.enabled)
        findChild(main, "devTransferFrom").text = "owner-stable"
        findChild(main, "devTransferTo").text = "taker-stable"
        findChild(main, "devTransferAmount").text = "5000"
        verify(submit.enabled)
        testCase.calls = []
        submit.clicked()
        compare(testCase.calls.length, 1)
        compare(testCase.calls[0].action, "token_transfer")
        compare(testCase.calls[0].amount, "5000")
        verify(findChild(main, "devTransferStatus").text.length > 0)
        main.destroy()
    }
}
