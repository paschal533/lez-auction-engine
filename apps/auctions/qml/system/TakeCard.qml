import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format

// The take card of the live surplus or debt auction (board v5). The amount (stable, whole units) is quoted
// with `quote_take_surplus` / `quote_take_debt` {amount} a moment after typing stops; the card shows the reply
// as returned: `filled` stable, `tokens` protocol tokens, `lot_left_after` and `limit`, the slippage bound
// prepare sends (crates/auction_ffi/src/quotes.rs take).
//   surplus: the taker pays `tokens` protocol tokens from <label>-protocol, which the engine burns, and gets
//            `filled` stable in <label>-stable (programs/auction_engine/src/surplus.rs take_surplus);
//   debt:    the taker pays `filled` stable from <label>-stable, burned against unbacked debt, and gets
//            `tokens` newly minted protocol tokens in <label>-protocol (programs.md take_debt).
// Submit reads the quote again, confirms and sends {action: take_surplus | take_debt, payer, receiver, amount}.
// objectNames carry `prefix`: <prefix>Amount, <prefix>Pay, <prefix>Get, <prefix>LotAfter, <prefix>Limit,
// <prefix>Submit; the note is `noteName`.
InsetCard {
    id: root

    property var page: null
    property var backend: null
    property bool surplus: true
    property string prefix: surplus ? "takeSurplus" : "takeDebt"
    property string noteName: prefix + "Note"
    // The overview the panel shows: a new one (after an included transaction, a refresh) re-quotes the amount.
    property var overview: null

    readonly property string amountText: field.text
    readonly property bool validAmount: /^\d+$/.test(amountText) && Format.cmp(amountText, "0") > 0
    readonly property var quote: d.quote && d.quoteAmount === amountText ? d.quote : null
    readonly property string label: page ? page.activeAccount : ""
    readonly property string payer: label + (surplus ? "-protocol" : "-stable")
    readonly property string receiver: label + (surplus ? "-stable" : "-protocol")
    readonly property bool canTake: validAmount && !!quote && !!page && !!page.signer && !page.busy

    function amount(v) { return Format.amount(String(v), 0) }

    function noteFor(q) {
        if (!q) return ""
        const slip = Format.bpsPercentTrim(q.slippage_bps)
        return surplus
            ? qsTr("The %1 protocol tokens you pay are burned. You pay at most %2 (%3 % slippage).")
                  .arg(amount(q.tokens)).arg(amount(q.limit)).arg(slip)
            : qsTr("The %1 stable you pay is burned against unbacked debt; the %2 protocol tokens are newly minted. You get at least %3 (%4 % slippage).")
                  .arg(amount(q.filled)).arg(amount(q.tokens)).arg(amount(q.limit)).arg(slip)
    }

    padding: Theme.spacing.large
    spacing: Theme.spacing.small

    onOverviewChanged: if (validAmount) quoteTimer.restart()

    component Line: RowLayout {
        id: line
        property string label: ""
        property string value: ""
        property string unit: ""

        Layout.fillWidth: true
        Layout.minimumHeight: 22
        spacing: Theme.spacing.small
        LogosText {
            Layout.fillWidth: true
            text: line.label
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textTertiary
            elide: Text.ElideRight
        }
        LogosText {
            objectName: "takeRowValue"
            text: line.value.length > 0 ? Format.amount(line.value, 0) : "—"
            font.pixelSize: Theme.typography.primaryText
            font.weight: Theme.typography.weightMedium
            font.features: { "tnum": 1 }
            color: Theme.palette.text
        }
        LogosText {
            text: line.unit
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textSecondary
        }
    }

    component Side: ColumnLayout {
        id: side
        property string caption: ""
        property string valueName: ""
        property string value: ""
        property string unit: ""

        Layout.fillWidth: true
        spacing: 0
        LogosText {
            text: side.caption
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textTertiary
        }
        RowLayout {
            spacing: Theme.spacing.tiny
            LogosText {
                objectName: side.valueName
                text: side.value.length > 0 ? Format.amount(side.value, 0) : "—"
                font.pixelSize: Theme.typography.panelTitleText
                font.weight: Theme.typography.weightBold
                font.features: { "tnum": 1 }
                color: Theme.palette.text
            }
            LogosText {
                text: side.unit
                font.pixelSize: Theme.typography.primaryText
                color: Theme.palette.textSecondary
            }
        }
    }

    QtObject {
        id: d

        property var quote: null
        property string quoteAmount: ""
        property var notice: null
        property int generation: 0

        readonly property string view: root.surplus ? "quote_take_surplus" : "quote_take_debt"

        function load() {
            const amount = root.amountText
            const gen = ++generation
            notice = null
            if (!root.backend || !root.validAmount)
                return
            logos.watch(root.backend.view({ view: view, amount: amount }), function(reply) {
                if (gen !== generation) return
                if (!reply || reply.error) {
                    quote = null
                    notice = reply && reply.error ? reply : { error: qsTr("The engine sent no quote."), kind: "bad_reply" }
                    return
                }
                quote = reply
                quoteAmount = amount
            }, function(err) {
                if (gen === generation) notice = { error: String(err), kind: "ipc" }
            })
        }

        function take() {
            const amount = root.amountText
            const from = root.page.signer ? String(root.page.signer.id) : ""
            const payer = root.payer
            const receiver = root.receiver
            const surplus = root.surplus
            root.page.act({
                title: surplus ? qsTr("Take surplus lot") : qsTr("Take debt lot"),
                read: { view: view, amount: amount },
                check: function(q) {
                    d.quote = q
                    d.quoteAmount = amount
                    return true
                },
                dialog: function(q) {
                    return {
                        action: surplus ? "take_surplus" : "take_debt",
                        from: from,
                        pay: surplus ? { amount: String(q.tokens), decimals: 0, unit: qsTr("protocol") }
                                     : { amount: String(q.filled), decimals: 0, unit: qsTr("stable") },
                        get: surplus ? { amount: String(q.filled), decimals: 0, unit: qsTr("stable") }
                                     : { amount: String(q.tokens), decimals: 0, unit: qsTr("protocol") },
                        facts: [
                            surplus ? { label: qsTr("Max protocol in"), value: String(q.limit), decimals: 0, unit: qsTr("protocol") }
                                    : { label: qsTr("Min protocol out"), value: String(q.limit), decimals: 0, unit: qsTr("protocol") },
                            { label: qsTr("Lot left after"), value: String(q.lot_left_after), decimals: 0, unit: qsTr("stable") },
                            { label: qsTr("From"), value: payer },
                            { label: qsTr("To"), value: receiver }
                        ],
                        note: root.noteFor(q),
                        confirmLabel: surplus ? qsTr("Take surplus lot") : qsTr("Take debt lot")
                    }
                },
                // The slippage the confirmed quote was computed with, so its limit is the one sent. A number read
                // back from a reply crosses the module as a double (50.0), which the engine's u32 refuses: `| 0`
                // sends it as an integer.
                submit: function(q) {
                    return { action: surplus ? "take_surplus" : "take_debt", payer: payer, receiver: receiver, amount: amount,
                             slippage_bps: Number(q.slippage_bps) | 0 }
                }
            })
        }
    }

    Timer {
        id: quoteTimer
        objectName: root.prefix + "QuoteTimer"
        interval: 300
        onTriggered: d.load()
    }

    LogosText {
        Layout.fillWidth: true
        text: root.surplus ? qsTr("Take the surplus lot") : qsTr("Take the debt lot")
        font.pixelSize: Theme.typography.primaryText
        font.weight: Theme.typography.weightBold
        color: Theme.palette.text
    }

    LogosText {
        // Surplus: the stable the taker receives; debt: the stable the taker pays (amount_coin either way).
        text: root.surplus ? qsTr("Stable to take") : qsTr("Stable to pay")
        font.pixelSize: Theme.typography.secondaryText
        color: Theme.palette.textTertiary
    }
    LogosTextField {
        id: field
        objectName: root.prefix + "Amount"
        Layout.fillWidth: true
        Layout.preferredHeight: 44
        placeholderText: "0"
        validator: RegularExpressionValidator { regularExpression: /^\d{0,39}$/ }
        enabled: !!root.page && !root.page.busy
        onTextChanged: quoteTimer.restart()
        Component.onCompleted: {
            textInput.font.pixelSize = Theme.typography.subtitleText
            textInput.font.weight = Theme.typography.weightBold
            textInput.font.features = { "tnum": 1 }
        }
        Binding {
            target: field.backgroundItem.border
            property: "color"
            when: field.text.length > 0 && !root.validAmount
            value: Theme.palette.error
        }
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: Theme.spacing.medium
        Side {
            caption: qsTr("You pay")
            valueName: root.prefix + "Pay"
            value: !root.quote ? "" : root.surplus ? String(root.quote.tokens) : String(root.quote.filled)
            unit: root.surplus ? qsTr("protocol") : qsTr("stable")
        }
        LogosText {
            text: "→"
            font.pixelSize: Theme.typography.subtitleText
            color: Theme.palette.textTertiary
        }
        Side {
            caption: qsTr("You get")
            valueName: root.prefix + "Get"
            value: !root.quote ? "" : root.surplus ? String(root.quote.filled) : String(root.quote.tokens)
            unit: root.surplus ? qsTr("stable") : qsTr("protocol")
        }
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 0
        Line {
            objectName: root.prefix + "Limit"
            label: root.surplus ? qsTr("Max protocol in") : qsTr("Min protocol out")
            value: root.quote ? String(root.quote.limit) : ""
            unit: qsTr("protocol")
        }
        Line {
            objectName: root.prefix + "LotAfter"
            label: qsTr("Lot left after")
            value: root.quote ? String(root.quote.lot_left_after) : ""
            unit: qsTr("stable")
        }
    }

    LogosText {
        objectName: root.noteName
        Layout.fillWidth: true
        visible: !!root.quote
        wrapMode: Text.WordWrap
        text: root.noteFor(root.quote)
        font.pixelSize: Theme.typography.secondaryText
        font.features: { "tnum": 1 }
        color: Theme.palette.textSecondary
    }
    LogosText {
        objectName: root.prefix + "Holdings"
        Layout.fillWidth: true
        visible: root.label.length > 0
        wrapMode: Text.WordWrap
        text: qsTr("From %1 to %2.").arg(root.payer).arg(root.receiver)
        font.pixelSize: Theme.typography.secondaryText
        color: Theme.palette.textTertiary
    }

    StateNotice {
        objectName: root.prefix + "QuoteNotice"
        Layout.fillWidth: true
        prefix: qsTr("Quote")
        reply: d.notice
    }

    ActionButton {
        objectName: root.prefix + "Submit"
        primary: true
        Layout.fillWidth: true
        text: root.surplus ? qsTr("Take surplus lot") : qsTr("Take debt lot")
        enabled: root.canTake
        onClicked: d.take()
    }
}
