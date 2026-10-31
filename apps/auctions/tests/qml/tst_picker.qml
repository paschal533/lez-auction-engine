import QtQuick
import QtTest

import "../../qml/shell"
import "Fixtures.js" as Fixtures

// The account picker against the real accounts fixture (auction_engine accounts()).
TestCase {
    id: testCase
    name: "AccountPicker"
    width: 1100
    height: 720
    visible: true
    when: windowShown

    function initTestCase() {
        Qt.application.organization = "auctions-qml-tests"
        Qt.application.domain = "auctions.test"
    }

    Component {
        id: pickerComponent
        AccountPicker {
            x: 300
            y: 80
        }
    }

    SignalSpy {
        id: pickedSpy
        signalName: "picked"
    }

    function createPicker(rows, active) {
        const picker = createTemporaryObject(pickerComponent, testCase, { rows: rows, activeLabel: active })
        verify(picker !== null)
        return picker
    }

    function option(picker, label) {
        return findChild(picker.popup.contentItem, "accountOption_" + label)
    }

    function test_picker_lists_roles_and_hides_dev_keys() {
        const picker = createPicker(Fixtures.load("accounts"), "owner")
        findChild(picker, "accountPicker").clicked()
        tryCompare(picker.popup, "opened", true)

        const owner = option(picker, "owner")
        verify(owner !== null, "owner listed")
        compare(findChild(owner, "accountRole").text, "Opens positions, bids, claims leftover")
        compare(findChild(owner, "accountHolding").text, "owner-stable")
        verify(owner.selected, "the active account is marked")
        compare(findChild(option(picker, "keeper"), "accountRole").text, "Liquidates, earns the reward")
        compare(findChild(option(picker, "caller"), "accountRole").text, "Upkeep: settle, check, start auctions")
        compare(findChild(option(picker, "taker"), "accountRole").text, "Takes surplus and debt lots")
        // u128 balances are decimal strings, grouped without a double in between.
        compare(findChild(option(picker, "bidder"), "accountBalance").text, "1,000,000")
        compare(findChild(option(picker, "caller"), "accountHolding").text, "—")

        compare(option(picker, "engine-admin"), null, "dev key engine-admin is not listed")
        compare(option(picker, "oracle-src"), null, "dev key oracle-src is not listed")
        const note = findChild(picker.popup.contentItem, "accountPickerNote").text
        verify(note.indexOf("engine-admin") >= 0 && note.indexOf("oracle-src") >= 0, note)
    }

    function test_picking_an_account_emits_its_label_and_closes() {
        const picker = createPicker(Fixtures.load("accounts"), "owner")
        pickedSpy.target = picker
        pickedSpy.clear()
        findChild(picker, "accountPicker").clicked()
        tryCompare(picker.popup, "opened", true)
        option(picker, "keeper").clicked()
        compare(pickedSpy.count, 1)
        compare(pickedSpy.signalArguments[0][0], "keeper")
        tryCompare(picker.popup, "visible", false)
    }

    function test_button_shows_label_and_short_id() {
        const picker = createPicker(Fixtures.load("accounts"), "owner")
        compare(findChild(picker, "headerAccount").text, "owner")
        compare(findChild(picker, "headerAccountId").text, "7wx9…opKg")
    }

    function test_error_row_is_a_notice_never_an_account() {
        const picker = createPicker([{ error: "the node did not answer", kind: "chain" }], "owner")
        findChild(picker, "accountPicker").clicked()
        tryCompare(picker.popup, "opened", true)
        const notice = findChild(picker.popup.contentItem, "accountPickerError")
        verify(notice.visible)
        compare(notice.message, "the node did not answer")
        compare(option(picker, "undefined"), null)
        compare(findChild(picker, "headerAccountId").text, "")
    }
}
