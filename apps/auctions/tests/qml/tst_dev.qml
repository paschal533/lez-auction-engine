import QtQuick
import QtTest

import Logos.Controls
import Logos.Theme

import "../../qml/dev"
import "../../qml/common/Format.js" as Format
import "Fixtures.js" as Fixtures

// Dev tools (board v5) against a stub backend that answers with the real fixtures:
//   overview:     oracle price 0.5 (RAY), age 0 of a 60 min max, allowed range 0.25–1 (½ to 2 × R),
//                 host min CR 200 %;
//   quote_price:  the engine's verdict on publishing 0.5 (valid) and one position at CR 166.6 % now and after
//                 (R19: the publish writes nothing CR is judged by, so after equals now);
//   deployment:   a verified dev network (localnet, genesis ok).
// States no fixture has (an out-of-range price, a non-dev network) are variants of the real JSON.
TestCase {
    id: testCase
    name: "Dev"
    width: 1100
    height: 760
    visible: true
    when: windowShown

    readonly property string adminId: "HNQ2aVbCErw9h3dNXu8m7WhYZCsHCZGpeRsu6AqjSqFd"
    readonly property string position: "7xdCdVxS8WU8d7XyF32j3nR26AqA5QQcfWBSejgG6T2S"

    property var overviewReply: ({})
    property var priceQuote: ({})
    property var submitReply: ({})
    property var statusReply: ({})
    property var calls: []
    property var page: null

    function initTestCase() {
        Qt.application.organization = "auctions-qml-tests"
        Qt.application.domain = "auctions.test"
    }

    QtObject {
        id: backendStub
        property string network: "localnet"
        property bool devNetwork: true
        property string activeAccount: "host-admin"
        property bool walletOpen: true
        property bool nodeReachable: true
        property string lastError: ""
        function view(req) { testCase.calls.push({ call: "view", req: req }); return { call: "view", req: req } }
        function accounts() { return { call: "accounts" } }
        function rescan(req) { return { call: "rescan" } }
        function submit(req) { testCase.calls.push({ call: "submit", req: req }); return { call: "submit" } }
        function txStatus(id) { return { call: "txStatus", txId: id } }
    }

    QtObject {
        id: logos
        function module(name) { return backendStub }
        function watch(promise, ok, err) {
            if (!promise) return
            const copy = function(v) { return JSON.parse(JSON.stringify(v)) }
            switch (promise.call) {
            case "accounts": ok(Fixtures.load("accounts")); return
            case "view":
                switch (promise.req.view) {
                case "overview": ok(copy(testCase.overviewReply)); return
                case "quote_price": {
                    const q = copy(testCase.priceQuote)
                    q.price = String(promise.req.price)
                    ok(q)
                    return
                }
                case "live": ok(Fixtures.load("live")); return
                case "at_risk": ok(Fixtures.load("at_risk")); return
                case "settled": ok(Fixtures.load("settled")); return
                }
                ok({ error: "unknown view", kind: "bad_request" })
                return
            case "rescan": ok(Fixtures.load("scan_done")); return
            case "submit": ok(copy(testCase.submitReply)); return
            case "txStatus": ok(copy(testCase.statusReply)); return
            }
        }
    }

    Component {
        id: pageComponent
        DevPage {
            width: 1060
            height: 740
            backend: backendStub
        }
    }

    function init() {
        overviewReply = Fixtures.load("overview")
        priceQuote = Fixtures.load("quote_price")
        submitReply = { txId: "tx1", hash: "h", what: "create_oracle_price_account", step: 0, steps: 3,
                        tracking: { signer: adminId } }
        statusReply = { txId: "tx1", state: "pending", blocks_waited: 1, step: 0, steps: 3 }
        calls = []
        page = null
        backendStub.activeAccount = "host-admin"
        backendStub.devNetwork = true
    }

    function cleanup() {
        page = null
    }

    function cleanupTestCase() {
        gc()
    }

    function createPage(section, devNetwork) {
        page = createTemporaryObject(pageComponent, testCase, {
            accounts: Fixtures.load("accounts"),
            activeAccount: backendStub.activeAccount,
            devNetwork: devNetwork === undefined ? true : devNetwork,
            section: section || ""
        })
        verify(page !== null)
        return page
    }

    function readyPage(section) {
        const p = createPage(section)
        tryVerify(function() { return p.overview !== null })
        return p
    }

    function callsOf(kind, viewName) {
        return calls.filter(function(c) {
            return c.call === kind && (viewName === undefined || (c.req && c.req.view === viewName))
        })
    }

    function lastSubmit() {
        const s = callsOf("submit")
        verify(s.length > 0, "a submit was sent")
        return s[s.length - 1].req
    }

    function typePrice(p, text) {
        findChild(p, "devPrice").text = text
    }

    // ---- Brief tests -------------------------------------------------------------------------------

    // R22: the Dev tools tab exists only on a verified dev network; off one nothing is read or sent.
    function test_dev_hidden_off_dev_networks() {
        backendStub.devNetwork = false
        const component = Qt.createComponent("../../qml/Main.qml")
        compare(component.status, Component.Ready, component.errorString())
        const main = createTemporaryObject(component, testCase, { width: 1100, height: 720 })
        verify(main !== null)
        verify(!findChild(main, "tabDev").visible, "Dev tools tab hidden off a dev network")
        const devPage = findChild(main, "devPage")
        verify(!devPage.active, "the page is inactive")
        compare(callsOf("view", "quote_price").length, 0)
        compare(callsOf("submit").length, 0)

        // A page shown off a dev network says so and offers no action.
        calls = []
        const p = createPage("price", false)
        wait(50)
        compare(callsOf("view").length, 0, "no read off a dev network")
        verify(findChild(p, "devOffNotice").visible)
        verify(!findChild(p, "devPricePublish").enabled)
        verify(!findChild(p, "devDisableProtocolToken").enabled)

        // On a dev network the tab is back.
        backendStub.devNetwork = true
        tryVerify(function() { return findChild(main, "tabDev").visible })
    }

    // R17/R19: the "At <price>" list is quote_price's positions, as returned; nothing is scaled by the price.
    function test_affected_positions_from_quote() {
        const p = readyPage("price")
        typePrice(p, "0.5")
        tryVerify(function() { return callsOf("view", "quote_price").length > 0 }, 3000, "quote_price read")
        compare(callsOf("view", "quote_price")[0].req.price, "500000000000000000000000000")
        const row = findChild(p, "devAffected_" + position)
        tryVerify(function() { return row !== null && findChild(p, "devAffected_" + position) !== null })
        const shown = findChild(p, "devAffected_" + position)
        compare(findChild(shown, "devAffectedNow").text, "166.6%")
        compare(findChild(shown, "devAffectedAfter").text, "166.6%")
        compare(findChild(p, "devAffectedTitle").text, "At 0.5 collateral per stable")
        // 16,666 bps is below the 200 % minimum now and after: liquidatable already, none newly.
        compare(findChild(p, "devPriceNewlyLiquidatable").text, "0")
        verify(findChild(p, "devPriceExplainer").text.indexOf("redemption price") >= 0,
               "the copy says CR is judged at the redemption price")

        // A different quote is shown as returned (no price ratio applied in QML).
        priceQuote.positions[0].cr_bps_now = 21000
        priceQuote.positions[0].cr_bps_after = 15000
        typePrice(p, "0.6")
        tryVerify(function() {
            const r = findChild(p, "devAffected_" + position)
            return r && findChild(r, "devAffectedAfter").text === "150.0%"
        }, 3000)
        compare(findChild(findChild(p, "devAffected_" + position), "devAffectedNow").text, "210.0%")
        compare(findChild(p, "devPriceNewlyLiquidatable").text, "1")
    }

    // Outside the allowed range the field turns red, the error names the range, and the button reads
    // "Publish anyway" as a secondary action; the engine's own verdict (quote_price.reason) is shown too.
    function test_out_of_range_price_warns() {
        const p = readyPage("price")
        const field = findChild(p, "devPrice")
        const button = findChild(p, "devPricePublish")
        typePrice(p, "0.5")
        tryCompare(findChild(p, "devPriceRange"), "text", "In range")
        verify(!findChild(p, "devPriceError").visible)
        compare(button.text, "Publish price")
        compare(button.variant, LogosButton.Variant.Primary)

        priceQuote.valid = false
        priceQuote.reason = "engine error: OracleOutOfRange"
        typePrice(p, "1.5")
        tryCompare(findChild(p, "devPriceRange"), "text", "Out of range")
        const error = findChild(p, "devPriceError")
        verify(error.visible)
        verify(error.text.indexOf("0.25–1") >= 0, error.text)
        compare(String(field.backgroundItem.border.color), String(Theme.palette.error))
        compare(button.text, "Publish anyway")
        compare(button.variant, LogosButton.Variant.Secondary)
        tryVerify(function() { return findChild(p, "devPriceVerdict").text.indexOf("OracleOutOfRange") >= 0 }, 3000)
    }

    // The freeze trigger never sends on the first click: the dialog repeats the warning, Cancel sends nothing.
    function test_disable_token_needs_confirmation() {
        const p = readyPage("token")
        const button = findChild(p, "devDisableProtocolToken")
        tryVerify(function() { return button.enabled }, 3000)
        verify(findChild(p, "devTokenWarning").visible)
        button.clicked()
        const dialog = findChild(p, "devTxDialog")
        tryCompare(dialog, "opened", true)
        compare(callsOf("submit").length, 0, "nothing sent before Confirm")
        compare(dialog.request.action, "disable_protocol_token")
        verify(String(dialog.request.note).indexOf("Check solvency") >= 0, "the dialog repeats the warning")
        findChild(dialog, "devTxCancel").clicked()
        tryCompare(dialog, "opened", false)
        compare(callsOf("submit").length, 0, "Cancel sends nothing")

        submitReply = { txId: "tx2", hash: "h", what: "set_engine_params", step: 0, steps: 1, tracking: { signer: "x" } }
        statusReply = { txId: "tx2", state: "pending", blocks_waited: 0, step: 0, steps: 1 }
        button.clicked()
        tryCompare(dialog, "opened", true)
        findChild(dialog, "devTxConfirm").clicked()
        compare(JSON.stringify(lastSubmit()), JSON.stringify({ action: "disable_protocol_token", admin: "engine-admin" }))
        tryCompare(findChild(p, "devTxLane"), "state", "waiting")
    }

    // ---- Further behaviour ---------------------------------------------------------------------------

    // Publish signs with the three dev keys (oracle-src, the active account as host admin, engine-admin; the
    // fixture wallet's host admin is `host-admin`, the localnet's `admin`) and the lane
    // follows its three steps (R27).
    function test_publish_signs_three_ways_and_lane_has_three_stages() {
        const p = readyPage("price")
        typePrice(p, "1.9")
        const button = findChild(p, "devPricePublish")
        tryVerify(function() { return button.enabled }, 3000)
        button.clicked()
        const dialog = findChild(p, "devTxDialog")
        tryCompare(dialog, "opened", true)
        compare(dialog.request.action, "oracle_publish")
        dialog.confirmed()
        dialog.close()
        compare(JSON.stringify(lastSubmit()), JSON.stringify({
            action: "oracle_publish", price_source: "oracle-src", price: "1900000000000000000000000000",
            host_admin: "host-admin", engine_admin: "engine-admin"
        }))
        const lane = findChild(p, "devTxLane")
        tryCompare(lane, "state", "waiting")
        compare(lane.steps, 3)
        compare(findChild(lane, "devTxStages").stages.length, 3)
    }

    // The keeper refresh re-publishes the current price (overview.oracle.price_ray).
    function test_republish_current_uses_the_oracle_price() {
        const p = readyPage("price")
        findChild(p, "devPriceRepublish").activate()
        const dialog = findChild(p, "devTxDialog")
        tryCompare(dialog, "opened", true)
        dialog.confirmed()
        dialog.close()
        compare(lastSubmit().price, overviewReply.oracle.price_ray)
    }

    // R22: a dev action on a network that is not a dev network comes back `mismatch`, shown on the page.
    function test_mismatch_reply_is_shown() {
        const p = readyPage("token")
        submitReply = { error: "not a dev network", kind: "mismatch" }
        const button = findChild(p, "devDisableProtocolToken")
        tryVerify(function() { return button.enabled }, 3000)
        button.clicked()
        const dialog = findChild(p, "devTxDialog")
        tryCompare(dialog, "opened", true)
        dialog.confirmed()
        dialog.close()
        const notice = findChild(p, "devActionNotice")
        tryVerify(function() { return notice.shown })
        verify(notice.message.indexOf("not a dev network") >= 0)
        compare(findChild(p, "devTxLane").txId, "")
    }

    function test_list_has_three_items_and_switches_panels() {
        const p = readyPage("")
        for (const name of ["price", "transfer", "token"])
            verify(findChild(p, "devItem_" + name) !== null, name)
        compare(p.selected, "price")
        findChild(p, "devItem_transfer").clicked()
        compare(p.selected, "transfer")
        verify(findChild(p, "devTransferSubmit").visible)
    }
}
