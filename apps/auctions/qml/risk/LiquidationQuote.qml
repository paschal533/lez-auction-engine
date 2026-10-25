import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../shell"
import "../common/Format.js" as Format

// The quote card of the At-risk screen (board v4, right column). Every figure is a field of the `position`
// view's liquidation quote (the SDK's LiquidationQuoteView, what `auction-cli quote-liquidation` prints):
//   what the auction must raise: debt to cover (debt_principal), auction target (amount_to_raise);
//   where the collateral goes: seized, your reward (keeper_reward) with the engine's formula, to auction (lot).
// "Will end short" shows when the lot is worth less than the target at the quote price (lot × RAY <
// amount_to_raise × price_ray, a comparison of returned fields). The board's penalty, network fee and cap
// rows have no field in the quote or the views and are left out.
// States: healthy (button disabled, `liquidateReason`), price feed stale / out of range / collateral paused by
// the admin (`riskFeedNotice`, "Liquidations paused"), oracle unknown. Other refusals (`refused` in the view,
// a failed quote or submit) show in `liquidateNotice`, never as node trouble unless their kind is `chain`.
// Liquidating: a fresh quote_liquidation → TxDialog → submit (the active account is the keeper; the reward
// goes to its `<label>-coll` holding) → TxLane in place of the button. laneDone() fires when it is done.
InsetCard {
    id: root
    objectName: "liquidationQuote"

    property var backend: null
    // The `position` view: health fields, `quote` (or null) and `refused` (or null).
    property var position: null
    // overview.oracle {price_ray, fresh, age_ms, max_age_ms, range_lo_ray, range_hi_ray}, or null while unknown.
    property var oracle: null
    property var accounts: []
    property string activeAccount: ""

    readonly property var quote: position && position.quote ? position.quote : null
    readonly property string refused: position && position.refused ? String(position.refused) : ""
    // The engine error a refusal names ("the engine would reject this: PriceFeedStale (code 21)").
    readonly property string refusedName: {
        const m = /would reject this: (\w+) \(code \d+\)/.exec(refused)
        return m ? m[1] : ""
    }
    readonly property bool hasPosition: !!(position && position.position)
    readonly property bool healthy: hasPosition && !position.liquidatable
                                    && position.cr_bps !== null && position.cr_bps !== undefined
    readonly property bool oracleUnknown: !oracle
    readonly property bool feedStale: !!(oracle && oracle.fresh === false) || refusedName === "PriceFeedStale"
    readonly property bool feedOutOfRange: (!!oracle && Format.isDigits(String(oracle.price_ray))
                                            && (Format.cmp(String(oracle.price_ray), String(oracle.range_lo_ray)) < 0
                                                || Format.cmp(String(oracle.price_ray), String(oracle.range_hi_ray)) > 0))
                                           || refusedName === "PriceFeedOutOfRange"
    readonly property bool adminPaused: refusedName === "Paused"
    readonly property bool paused: feedStale || feedOutOfRange || adminPaused
    readonly property bool laneBusy: lane.visible && lane.state === "waiting"
    readonly property bool canLiquidate: hasPosition && !!quote && !!position.liquidatable && !paused
                                         && !oracleUnknown && d.keeper !== null && !d.quoting && !laneBusy
    readonly property string rayOne: "1000000000000000000000000000"
    readonly property bool endsShort: {
        if (!quote) return false
        const lotValue = Format.mul(String(quote.lot), rayOne)
        const targetValue = Format.mul(String(quote.amount_to_raise), String(quote.price_ray))
        return lotValue.length > 0 && targetValue.length > 0 && Format.cmp(lotValue, targetValue) < 0
    }

    signal laneDone()

    padding: Theme.spacing.large
    spacing: Theme.spacing.medium

    onPositionChanged: {
        const id = position && position.position ? String(position.position) : ""
        if (id !== d.positionId) {
            d.positionId = id
            d.notice = null
        }
    }

    // A label / value line; the value is `liqRowValue` inside the line's objectName.
    component QuoteLine: RowLayout {
        id: line
        property string label: ""
        property string value: ""
        property string unit: ""
        property bool strong: false
        property color valueColor: Theme.palette.text

        Layout.fillWidth: true
        Layout.minimumHeight: 22
        spacing: Theme.spacing.small

        LogosText {
            Layout.fillWidth: true
            text: line.label
            font.pixelSize: Theme.typography.secondaryText
            color: line.strong ? Theme.palette.textSecondary : Theme.palette.textTertiary
            elide: Text.ElideRight
        }
        LogosText {
            objectName: "liqRowValue"
            text: line.value.length > 0 ? Format.amount(line.value, 0) : "—"
            font.pixelSize: Theme.typography.primaryText
            font.weight: line.strong ? Theme.typography.weightBold : Theme.typography.weightMedium
            font.features: { "tnum": 1 }
            color: line.valueColor
        }
        LogosText {
            visible: text.length > 0
            text: line.unit
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textSecondary
        }
    }

    component Divider: Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: 1
        color: Theme.palette.borderSubtle
    }

    QtObject {
        id: d

        property string positionId: ""
        property bool quoting: false
        // The last failed quote or submit, {error, kind}.
        property var notice: null

        readonly property var keeper: {
            const rows = root.accounts || []
            for (let i = 0; i < rows.length; i++)
                if (rows[i] && rows[i].label === root.activeAccount && !rows[i].error) return rows[i]
            return null
        }

        // The refusal the view carries, shown unless it is the healthy answer or a pause the notice explains.
        readonly property var viewRefusal: root.refused.length > 0 && !root.healthy && !root.paused
                                           ? { error: root.refused, kind: "refused" } : null

        function minutes(ms) { return Math.floor(Number(ms) / 60000) }

        function submitRequest(quote) {
            return {
                action: "liquidate",
                position: String(quote.position),
                keeper: root.activeAccount,
                keeper_holding: root.activeAccount + "-coll"
            }
        }

        function dialogRequest(quote) {
            return {
                action: "liquidate",
                subtitle: qsTr("Position %1").arg(Format.shortId(String(quote.position))),
                from: keeper ? String(keeper.id) : "",
                facts: [
                    { label: qsTr("Auction"), value: Format.shortId(String(quote.auction)) },
                    { label: qsTr("Debt to cover"), value: String(quote.debt_principal), decimals: 0, unit: qsTr("stable") },
                    { label: qsTr("Auction target"), value: String(quote.amount_to_raise), decimals: 0, unit: qsTr("stable"),
                      strong: true },
                    { label: qsTr("Collateral seized"), value: String(quote.seized), decimals: 0, unit: qsTr("collateral") },
                    { label: qsTr("Your reward"), value: String(quote.keeper_reward), decimals: 0, unit: qsTr("collateral") },
                    { label: qsTr("Goes to auction"), value: String(quote.lot), decimals: 0, unit: qsTr("collateral") }
                ],
                note: qsTr("The reward goes to %1-coll. The auction appears under Live auctions after a rescan.").arg(root.activeAccount),
                confirmLabel: qsTr("Liquidate"),
                noun: qsTr("liquidation")
            }
        }

        // A fresh quote for the position, then the confirmation.
        function liquidate() {
            if (!root.canLiquidate || !root.backend)
                return
            notice = null
            if (lane.state === "done" || lane.state === "dropped")
                lane.txId = ""
            quoting = true
            logos.watch(root.backend.view({ view: "quote_liquidation", position: String(root.position.position) }),
                        function(quote) {
                quoting = false
                if (!quote || quote.error) {
                    notice = quote && quote.error ? quote : { error: qsTr("The engine sent no quote."), kind: "bad_reply" }
                    return
                }
                dialog.quote = quote
                dialog.request = dialogRequest(quote)
                dialog.open()
            }, function(err) {
                quoting = false
                notice = { error: String(err), kind: "ipc" }
            })
        }

        function submit() {
            const quote = dialog.quote
            if (!root.backend || !quote)
                return
            logos.watch(root.backend.submit(submitRequest(quote)), function(reply) {
                if (!reply || reply.error) {
                    notice = reply && reply.error ? reply : { error: qsTr("The engine sent no reply."), kind: "bad_reply" }
                    return
                }
                lane.submitReply = reply
                lane.request = { from: keeper ? String(keeper.id) : "" }
                lane.txId = String(reply.txId)
            }, function(err) {
                notice = { error: String(err), kind: "ipc" }
            })
        }
    }

    RowLayout {
        Layout.fillWidth: true
        LogosText {
            Layout.fillWidth: true
            text: qsTr("Liquidation quote")
            font.pixelSize: Theme.typography.primaryText
            font.weight: Theme.typography.weightBold
            color: Theme.palette.text
        }
        LogosText {
            objectName: "liqAuction"
            visible: !!root.quote
            text: root.quote ? qsTr("auction %1").arg(Format.shortId(String(root.quote.auction))) : ""
            font.pixelSize: Theme.typography.secondaryText
            font.family: Theme.typography.mono
            color: Theme.palette.textTertiary
        }
    }

    // ---- What the auction must raise (rows only while there is a quote) ----------------------------
    ColumnLayout {
        Layout.fillWidth: true
        visible: !!root.quote
        spacing: 0
        QuoteLine {
            objectName: "liqDebt"
            label: qsTr("Debt to cover")
            value: root.quote ? String(root.quote.debt_principal) : ""
            unit: qsTr("stable")
        }
        QuoteLine {
            objectName: "liqTarget"
            label: qsTr("Auction target")
            value: root.quote ? String(root.quote.amount_to_raise) : ""
            unit: qsTr("stable")
            strong: true
        }
    }

    Divider { visible: !!root.quote }

    // ---- Where the collateral goes ----------------------------------------------------------------
    ColumnLayout {
        Layout.fillWidth: true
        visible: !!root.quote
        spacing: 0
        QuoteLine {
            objectName: "liqSeized"
            label: qsTr("Collateral seized")
            value: root.quote ? String(root.quote.seized) : ""
            unit: qsTr("collateral")
        }
        QuoteLine {
            objectName: "liqReward"
            label: qsTr("Your reward")
            value: root.quote ? String(root.quote.keeper_reward) : ""
            unit: qsTr("collateral")
            valueColor: Theme.palette.accentOrange
        }
        LogosText {
            objectName: "liqRewardNote"
            Layout.fillWidth: true
            Layout.bottomMargin: Theme.spacing.tiny
            wrapMode: Text.WordWrap
            // The engine's formula: crates/auction_core/src/liquidation.rs (quote_liquidation).
            text: qsTr("min(tip + chip × debt, reward cap, penalty) in stable, paid in collateral at the quote price, rounded down; none when the debt is below the debt floor. It comes out of the penalty, so liquidating your own position never pays.")
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textTertiary
        }
        QuoteLine {
            objectName: "liqLot"
            label: qsTr("Goes to auction")
            value: root.quote ? String(root.quote.lot) : ""
            unit: qsTr("collateral")
            strong: true
        }
    }

    LogosText {
        objectName: "liqEndShort"
        Layout.fillWidth: true
        visible: root.endsShort
        wrapMode: Text.WordWrap
        textFormat: Text.StyledText
        text: "<b>" + qsTr("Will end short.") + "</b> "
              + qsTr("At the quote price the lot is worth less than the auction target, so the auction cannot raise all of it.")
        font.pixelSize: Theme.typography.secondaryText
        color: Theme.palette.warning
    }

    LogosNotice {
        objectName: "riskFeedNotice"
        Layout.fillWidth: true
        severity: LogosNotice.Warning
        shown: root.hasPosition && (root.paused || root.oracleUnknown)
        title: {
            if (root.feedStale) {
                if (!root.oracle) return qsTr("Collateral paused")
                const over = d.minutes(Number(root.oracle.age_ms) - Number(root.oracle.max_age_ms))
                return over > 0 ? qsTr("Collateral paused · %1m").arg(over) : qsTr("Collateral paused")
            }
            if (root.feedOutOfRange) return qsTr("Price feed out of range")
            if (root.adminPaused) return qsTr("Collateral paused")
            return root.oracleUnknown ? qsTr("Oracle unknown") : ""
        }
        message: {
            if (root.feedStale)
                return root.oracle
                       ? qsTr("The price feed is %1m old (max %2m). Liquidations and bids are paused until it updates.")
                             .arg(d.minutes(root.oracle.age_ms)).arg(d.minutes(root.oracle.max_age_ms))
                       : qsTr("The price feed is stale. Liquidations and bids are paused until it updates.")
            if (root.feedOutOfRange)
                return root.oracle
                       ? qsTr("Market %1 is outside %2–%3 collateral per stable. Liquidations and bids stay paused until it is back in range.")
                             .arg(Format.rayPrice(String(root.oracle.price_ray)))
                             .arg(Format.rayPrice(String(root.oracle.range_lo_ray)))
                             .arg(Format.rayPrice(String(root.oracle.range_hi_ray)))
                       : qsTr("The market price is outside the allowed range. Liquidations and bids stay paused until it is back in range.")
            if (root.adminPaused)
                return qsTr("The engine admin paused this collateral type. Liquidations and bids wait until it is unpaused.")
            return root.oracleUnknown
                   ? qsTr("The oracle could not be read, so the price feed's state is unknown. Open the tab again or rescan to retry.")
                   : ""
        }
    }

    StateNotice {
        objectName: "liquidateNotice"
        Layout.fillWidth: true
        prefix: qsTr("Liquidation")
        reply: d.notice ? d.notice : d.viewRefusal
    }

    LogosButton {
        objectName: "liquidateSubmit"
        Layout.fillWidth: true
        Layout.preferredHeight: 40
        visible: !root.laneBusy
        radius: Theme.spacing.radiusLarge
        variant: LogosButton.Variant.Primary
        enabled: root.canLiquidate
        text: root.oracleUnknown ? qsTr("Oracle unknown")
              : root.paused ? qsTr("Liquidations paused")
              : d.keeper === null ? qsTr("Pick the signing account")
              : d.quoting ? qsTr("Quoting…") : qsTr("Liquidate & start auction")
        onClicked: d.liquidate()
        // Primary buttons carry a black label (contrast on the orange fill), 14 px bold as on the board.
        Component.onCompleted: {
            labelItem.color = Qt.binding(function() {
                return enabled ? Theme.palette.backgroundBlack : Theme.palette.textMuted
            })
            labelItem.font.pixelSize = Theme.typography.primaryText
            labelItem.font.weight = Theme.typography.weightBold
        }
    }

    LogosText {
        objectName: "liquidateReason"
        Layout.fillWidth: true
        visible: root.healthy
        wrapMode: Text.WordWrap
        horizontalAlignment: Text.AlignHCenter
        text: !root.healthy ? ""
              : (Number(root.position.cr_bps) > Number(root.position.min_cr_bps)
                 ? qsTr("CR %1% is above the %2% minimum.") : qsTr("CR %1% is not below the %2% minimum."))
                    .arg(Format.bpsPercent(root.position.cr_bps, 1))
                    .arg(Format.bpsPercentTrim(root.position.min_cr_bps))
        font.pixelSize: Theme.typography.secondaryText
        font.features: { "tnum": 1 }
        color: Theme.palette.textSecondary
    }

    TxLane {
        id: lane
        Layout.fillWidth: true
        visible: txId.length > 0
        backend: root.backend
        accounts: root.accounts
        onStateChanged: if (state === "done") root.laneDone()
        onRequote: d.liquidate()
    }

    TxDialog {
        id: dialog
        onConfirmed: d.submit()
    }
}
