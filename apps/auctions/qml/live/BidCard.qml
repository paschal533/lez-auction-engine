import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../shell"
import "../common/Format.js" as Format

// The bid card of the Live screen (board v3 right column, states strip): oracle price with a Fresh/Stale badge
// and freshness bar; "You pay" (`bidAmount`) with "Max to finish" (`bidMax`, fills what is left to raise);
// "You get" (`bidYouGet`) from a live `quote_bid` with the slippage floor and Edit; the Private bid switch,
// disabled ("Public transactions only"); and the Primary button (`bidSubmit`).
// States: a stale oracle or a paused auction disables the button ("Bidding paused"); an oracle that could not
// be read disables it too ("Oracle unknown"); an amount over what is left to raise turns the field red with
// "Use N" (`bidUseMax`); an active account without a `<label>-stable` holding cannot pay (`bidHoldingHint`).
// Placing a bid: a fresh quote_bid → TxDialog (its facts name the instruction, bid or bid_with_settlement)
// → submit → TxLane in place of the button. Failures of the quote or the submit (`refused` included) show in
// `bidNotice`, never as node trouble. laneDone() fires when the lane reaches done.
InsetCard {
    id: root
    objectName: "bidCard"

    property var backend: null
    // The `auction` view of the selected auction.
    property var auction: null
    // overview.oracle {price_ray, fresh, age_ms, max_age_ms}, or null while unknown.
    property var oracle: null
    // auction_engine accounts() rows and the label that signs.
    property var accounts: []
    property string activeAccount: ""

    readonly property bool hasAuction: !!(auction && auction.id)
    readonly property string leftToRaise: hasAuction ? Format.subtract(String(auction.target), String(auction.raised)) : "0"
    readonly property string amount: amountField.text
    readonly property bool amountValid: Format.isDigits(amount) && Format.cmp(amount, "0") > 0
    readonly property bool overMax: amountValid && Format.cmp(amount, leftToRaise) > 0
    readonly property string previewAmount: overMax ? leftToRaise : amount
    readonly property bool oracleUnknown: !oracle
    readonly property bool oracleStale: !!(oracle && oracle.fresh === false)
    readonly property bool paused: !!(hasAuction && auction.paused)
    readonly property bool blocked: oracleUnknown || oracleStale || paused
    readonly property bool laneBusy: lane.visible && lane.state === "waiting"
    // The SDK's picker convention (crates/auction_sdk/src/views/accounts.rs): `<label>-stable` is the row's
    // stable holding, and `holding` names it when it exists.
    readonly property bool hasStableHolding: d.bidder !== null && d.bidder.holding === activeAccount + "-stable"
    readonly property bool canBid: hasAuction && !blocked && amountValid && !overMax && !d.quoting
                                   && !laneBusy && hasStableHolding && d.slippageValid
    property alias slippageBps: d.slippageBps

    signal laneDone()

    implicitWidth: 300
    padding: Theme.spacing.large
    spacing: Theme.spacing.medium

    onAuctionChanged: {
        // Another auction: the old quote and notice no longer apply.
        const id = auction && auction.id ? String(auction.id) : ""
        if (id !== d.auctionId) {
            d.auctionId = id
            d.preview = null
            d.notice = null
        }
        previewTimer.restart()
    }
    onPreviewAmountChanged: previewTimer.restart()
    onSlippageBpsChanged: previewTimer.restart()

    QtObject {
        id: d

        readonly property int defaultSlippageBps: 50
        property string auctionId: ""
        property int slippageBps: defaultSlippageBps
        // False while the slippage field holds something that is not 0-99.99 (bidding is disabled then).
        property bool slippageValid: true
        property var preview: null
        property int previewGeneration: 0
        property bool quoting: false
        // The last failed quote or submit, {error, kind}.
        property var notice: null
        property bool editingSlippage: false

        readonly property var bidder: {
            const rows = root.accounts || []
            for (let i = 0; i < rows.length; i++)
                if (rows[i] && rows[i].label === root.activeAccount && !rows[i].error) return rows[i]
            return null
        }
        readonly property string slippageText: (slippageBps / 100) + "%"

        function quoteRequest(coinIn) {
            return { view: "quote_bid", auction: String(root.auction.id), coin_in_max: coinIn,
                     slippage_bps: slippageBps }
        }

        function refreshPreview() {
            const gen = ++previewGeneration
            if (!root.backend || !root.hasAuction || !Format.isDigits(root.previewAmount)
                    || Format.cmp(root.previewAmount, "0") <= 0) {
                preview = null
                return
            }
            logos.watch(root.backend.view(quoteRequest(root.previewAmount)), function(reply) {
                if (gen === previewGeneration) preview = reply
            }, function(err) {
                if (gen === previewGeneration) preview = { error: String(err), kind: "ipc" }
            })
        }

        // Holdings by the SDK's label convention (crates/auction_sdk/src/views/accounts.rs): <label>-stable
        // pays, <label>-coll receives. The amount is the one the confirmed quote was made for.
        function submitRequest(quote) {
            return {
                action: "bid",
                auction: String(quote.auction),
                coin_in_max: String(quote.coin_in_max),
                bidder_stable: root.activeAccount + "-stable",
                bidder_collateral: root.activeAccount + "-coll",
                slippage_bps: slippageBps,
                min_out: String(quote.min_out)
            }
        }

        function dialogRequest(quote) {
            const settles = quote.instruction === "bid_with_settlement"
            return {
                action: "bid",
                subtitle: qsTr("Auction %1").arg(Format.shortId(String(quote.auction))),
                from: bidder ? String(bidder.id) : "",
                pay: { amount: String(quote.charged), decimals: 0, unit: qsTr("stable") },
                get: { amount: String(quote.collateral_out), decimals: 0, unit: qsTr("collateral"),
                       note: qsTr("at least %1 · %2 slippage").arg(Format.amount(String(quote.min_out), 0))
                                                              .arg(slippageText) },
                facts: [
                    { label: qsTr("Instruction"), value: String(quote.instruction) },
                    { label: qsTr("Discount"), value: Format.rayPercent(String(quote.discount_ray), 4) + "%" },
                    { label: qsTr("Price"), value: Format.rayPrice(String(quote.price_ray)),
                      unit: qsTr("collateral per stable") },
                    { label: qsTr("Debt part"), value: String(quote.debt_part), decimals: 0 },
                    { label: qsTr("Penalty part"), value: String(quote.penalty_part), decimals: 0 },
                    { label: qsTr("Left to raise after"),
                      value: Format.subtract(String(quote.remaining_target), String(quote.charged)), decimals: 0 },
                    { label: qsTr("Collateral left after"),
                      value: Format.subtract(String(quote.collateral_left), String(quote.collateral_out)), decimals: 0 }
                ],
                note: settles
                      ? qsTr("This bid raises the rest of the target, so it settles the auction in the same transaction (bid_with_settlement).")
                      : "",
                confirmLabel: qsTr("Confirm bid"),
                noun: qsTr("bid")
            }
        }

        // A fresh quote for the amount, then the confirmation.
        function placeBid() {
            if (!root.canBid || !root.backend)
                return
            notice = null
            // A new bid starts a new lane: a finished one's outcome must not read as this bid's. An unknown
            // outcome stays (its lock is kept; the engine refuses a second send by that signer as in_flight).
            if (lane.state === "done" || lane.state === "dropped")
                lane.txId = ""
            quoting = true
            logos.watch(root.backend.view(quoteRequest(root.amount)), function(quote) {
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
                lane.request = { from: bidder ? String(bidder.id) : "" }
                lane.txId = String(reply.txId)
            }, function(err) {
                notice = { error: String(err), kind: "ipc" }
            })
        }
    }

    Timer {
        id: previewTimer
        interval: 300
        onTriggered: d.refreshPreview()
    }

    // ---- Oracle -----------------------------------------------------------------------------------
    ColumnLayout {
        Layout.fillWidth: true
        spacing: Theme.spacing.tiny

        RowLayout {
            Layout.fillWidth: true
            LogosText {
                objectName: "bidOraclePrice"
                Layout.fillWidth: true
                text: root.oracle
                      ? qsTr("1 stable = %1 collateral").arg(Format.rayPrice(String(root.oracle.price_ray)))
                      : qsTr("Oracle unknown")
                font.pixelSize: Theme.typography.primaryText
                font.weight: Theme.typography.weightMedium
                font.features: { "tnum": 1 }
                color: Theme.palette.text
                elide: Text.ElideRight
            }
            LogosBadge {
                objectName: "bidOracleBadge"
                text: root.oracleUnknown ? qsTr("Unknown") : root.oracleStale ? qsTr("Stale") : qsTr("Fresh")
                color: root.oracleUnknown || root.oracleStale ? Theme.palette.warning : Theme.palette.success
            }
        }
        RowLayout {
            Layout.fillWidth: true
            visible: !!root.oracle
            spacing: Theme.spacing.small
            LogosText {
                objectName: "bidOracleAge"
                text: root.oracle ? qsTr("Oracle · %1m ago").arg(Math.floor(Number(root.oracle.age_ms) / 60000)) : ""
                font.pixelSize: Theme.typography.secondaryText
                font.features: { "tnum": 1 }
                color: root.oracleStale ? Theme.palette.warning : Theme.palette.textSecondary
            }
            LogosProgressBar {
                Layout.fillWidth: true
                Layout.preferredHeight: 4
                from: 0
                to: 1
                value: root.oracle && Number(root.oracle.max_age_ms) > 0
                       ? Math.min(1, Number(root.oracle.age_ms) / Number(root.oracle.max_age_ms)) : 0
                fillColor: root.oracleStale ? Theme.palette.warning : Theme.palette.success
            }
            LogosText {
                text: root.oracle ? qsTr("max %1m").arg(Math.floor(Number(root.oracle.max_age_ms) / 60000)) : ""
                font.pixelSize: Theme.typography.secondaryText
                font.features: { "tnum": 1 }
                color: Theme.palette.textSecondary
            }
        }
    }

    LogosNotice {
        objectName: "bidPausedNotice"
        Layout.fillWidth: true
        severity: LogosNotice.Warning
        title: qsTr("Bidding paused")
        shown: root.blocked
        message: root.oracleUnknown
                 ? qsTr("The oracle could not be read, so the price feed's freshness is unknown. Open the tab again or rescan to retry.")
                 : root.oracleStale || (root.paused && String(root.auction.paused) === "PriceFeedStale")
                 ? qsTr("The price feed is older than %1 minutes. Bids reopen when the oracle updates.")
                       .arg(root.oracle ? Math.floor(Number(root.oracle.max_age_ms) / 60000) : 60)
                 : (root.paused ? qsTr("The engine would refuse a bid now: %1.").arg(String(root.auction.paused)) : "")
    }

    // ---- You pay ----------------------------------------------------------------------------------
    ColumnLayout {
        Layout.fillWidth: true
        spacing: Theme.spacing.tiny

        RowLayout {
            Layout.fillWidth: true
            LogosText {
                Layout.fillWidth: true
                text: qsTr("You pay")
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textTertiary
            }
            LogosText {
                objectName: "bidBalance"
                text: qsTr("Balance %1").arg(d.bidder && d.bidder.stable_balance !== null && d.bidder.stable_balance !== undefined
                                             ? Format.amount(String(d.bidder.stable_balance), 0) : "—")
                font.pixelSize: Theme.typography.secondaryText
                font.features: { "tnum": 1 }
                color: Theme.palette.textTertiary
            }
        }

        LogosTextField {
            id: amountField
            objectName: "bidAmount"
            Layout.fillWidth: true
            Layout.preferredHeight: 44
            rightPadding: unitLabel.implicitWidth + 2 * Theme.spacing.medium
            placeholderText: "0"
            validator: RegularExpressionValidator { regularExpression: /^\d{0,39}$/ }
            enabled: !root.laneBusy
            onTextChanged: d.notice = null
            Component.onCompleted: {
                textInput.font.pixelSize = Theme.typography.subtitleText
                textInput.font.weight = Theme.typography.weightBold
                textInput.font.features = { "tnum": 1 }
            }
            // Over the max the field turns red (board v3 "Over the max").
            Binding {
                target: amountField.backgroundItem.border
                property: "color"
                value: root.overMax ? Theme.palette.error
                       : (amountField.textInput.activeFocus ? Theme.palette.overlayOrange : Theme.palette.backgroundElevated)
            }

            LogosText {
                id: unitLabel
                anchors.right: parent.right
                anchors.rightMargin: Theme.spacing.medium
                anchors.verticalCenter: parent.verticalCenter
                text: qsTr("stable")
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textSecondary
            }
        }

        RowLayout {
            Layout.fillWidth: true
            visible: !root.overMax && !root.laneBusy
            LogosText {
                objectName: "bidMin"
                Layout.fillWidth: true
                // Takes what Max to finish leaves and wraps there.
                Layout.preferredWidth: 0
                wrapMode: Text.WordWrap
                // R31: the engine refuses a smaller charge (BelowMinBid) unless the bid ends the auction.
                text: root.hasAuction && root.auction.min_bid !== undefined && root.auction.min_bid !== null
                      ? qsTr("Min %1 (not for the final bid)").arg(Format.amount(String(root.auction.min_bid), 0)) : ""
                font.pixelSize: Theme.typography.secondaryText
                font.features: { "tnum": 1 }
                color: Theme.palette.textTertiary
            }
            LogosLink {
                objectName: "bidMax"
                enabled: root.hasAuction && !root.laneBusy
                text: qsTr("Max to finish %1").arg(Format.amount(root.leftToRaise, 0))
                underline: false
                font.pixelSize: Theme.typography.secondaryText
                onActivated: amountField.text = root.leftToRaise
            }
        }

        RowLayout {
            Layout.fillWidth: true
            visible: root.overMax
            spacing: Theme.spacing.tiny
            LogosText {
                objectName: "bidOverMax"
                Layout.fillWidth: true
                visible: root.overMax
                wrapMode: Text.WordWrap
                text: qsTr("Only %1 stable is left to raise.").arg(Format.amount(root.leftToRaise, 0))
                font.pixelSize: Theme.typography.secondaryText
                font.features: { "tnum": 1 }
                color: Theme.palette.error
            }
            LogosLink {
                objectName: "bidUseMax"
                text: qsTr("Use %1").arg(Format.amount(root.leftToRaise, 0))
                underline: false
                font.pixelSize: Theme.typography.secondaryText
                font.weight: Theme.typography.weightBold
                onActivated: amountField.text = root.leftToRaise
            }
        }
    }

    // ---- You get ----------------------------------------------------------------------------------
    InsetCard {
        id: youGet
        Layout.fillWidth: true
        // Collapsed while a bid is in flight, so the lane fits the 720 px plugin card.
        visible: !root.laneBusy
        color: Theme.palette.backgroundSecondary
        border.width: 0
        padding: Theme.spacing.medium
        spacing: 2

        readonly property var quote: d.preview && !d.preview.error ? d.preview : null

        LogosText {
            text: root.overMax ? qsTr("You'd get at %1 stable").arg(Format.amount(root.leftToRaise, 0)) : qsTr("You get")
            font.pixelSize: Theme.typography.secondaryText
            font.features: { "tnum": 1 }
            color: Theme.palette.textTertiary
        }
        RowLayout {
            spacing: Theme.spacing.tiny
            LogosText {
                objectName: "bidYouGet"
                text: youGet.quote ? "≈ " + Format.amount(String(youGet.quote.collateral_out), 0) : "≈ —"
                font.pixelSize: Theme.typography.panelTitleText
                font.weight: Theme.typography.weightBold
                font.features: { "tnum": 1 }
                color: Theme.palette.text
            }
            LogosText {
                text: qsTr("collateral")
                font.pixelSize: Theme.typography.primaryText
                font.weight: Theme.typography.weightMedium
                color: Theme.palette.textSecondary
            }
        }
        RowLayout {
            spacing: Theme.spacing.tiny
            LogosText {
                objectName: "bidMinOut"
                text: youGet.quote
                      ? qsTr("At least %1 with %2 slippage").arg(Format.amount(String(youGet.quote.min_out), 0))
                                                           .arg(d.slippageText)
                      : qsTr("Slippage %1").arg(d.slippageText)
                font.pixelSize: Theme.typography.secondaryText
                font.features: { "tnum": 1 }
                color: Theme.palette.textSecondary
            }
            LogosLink {
                objectName: "bidSlippageEdit"
                text: d.editingSlippage ? qsTr("Done") : qsTr("Edit")
                underline: false
                font.pixelSize: Theme.typography.secondaryText
                onActivated: {
                    d.editingSlippage = !d.editingSlippage
                    if (d.editingSlippage) slippageField.forceActiveFocus()
                }
            }
        }
        RowLayout {
            visible: d.editingSlippage
            spacing: Theme.spacing.small
            LogosTextField {
                id: slippageField
                objectName: "bidSlippage"
                Layout.preferredWidth: 80
                Layout.preferredHeight: 32
                text: String(d.defaultSlippageBps / 100)
                validator: RegularExpressionValidator { regularExpression: /^\d{0,2}(\.\d{0,2})?$/ }
                font.features: { "tnum": 1 }
                onTextChanged: {
                    const bps = Format.percentToBps(text)
                    d.slippageValid = bps >= 0
                    if (bps >= 0) d.slippageBps = bps
                }
                Binding {
                    target: slippageField.backgroundItem.border
                    property: "color"
                    when: !d.slippageValid
                    value: Theme.palette.error
                }
            }
            LogosText {
                text: qsTr("% slippage")
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textSecondary
            }
        }
        LogosText {
            objectName: "bidSlippageError"
            Layout.fillWidth: true
            visible: !d.slippageValid
            wrapMode: Text.WordWrap
            text: qsTr("Enter a slippage from 0 to 99.99%.")
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.error
        }
        LogosText {
            objectName: "bidSettles"
            visible: !!youGet.quote && youGet.quote.instruction === "bid_with_settlement"
            text: qsTr("This bid settles the auction.")
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textSecondary
        }
        LogosText {
            objectName: "bidPreviewError"
            Layout.fillWidth: true
            visible: text.length > 0
            text: d.preview && d.preview.error ? qsTr("Quote: %1").arg(String(d.preview.error)) : ""
            wrapMode: Text.WordWrap
            font.pixelSize: Theme.typography.secondaryText
            color: d.preview && d.preview.kind === "chain" ? Theme.palette.warning : Theme.palette.error
        }
    }

    // ---- Private bid ------------------------------------------------------------------------------
    RowLayout {
        Layout.fillWidth: true
        // The lane takes its place while a bid is in flight (the card must fit the 720 px plugin card).
        visible: !root.laneBusy
        spacing: Theme.spacing.small
        LogosSwitch {
            objectName: "bidPrivate"
            enabled: false
            checked: false
        }
        ColumnLayout {
            spacing: 0
            LogosText {
                text: qsTr("Private bid")
                font.pixelSize: Theme.typography.primaryText
                font.weight: Theme.typography.weightMedium
                color: Theme.palette.text
            }
            LogosText {
                objectName: "bidPrivateNote"
                text: qsTr("Public transactions only")
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textSecondary
            }
        }
    }

    LogosText {
        objectName: "bidHoldingHint"
        Layout.fillWidth: true
        visible: d.bidder !== null && !root.hasStableHolding
        wrapMode: Text.WordWrap
        text: qsTr("%1 has no %1-stable holding to pay from. Pick an account that holds stable.").arg(root.activeAccount)
        font.pixelSize: Theme.typography.secondaryText
        color: Theme.palette.warning
    }

    StateNotice {
        objectName: "bidNotice"
        Layout.fillWidth: true
        prefix: qsTr("Bid")
        reply: d.notice
    }

    LogosButton {
        objectName: "bidSubmit"
        Layout.fillWidth: true
        Layout.preferredHeight: 40
        visible: !root.laneBusy
        radius: Theme.spacing.radiusLarge
        variant: LogosButton.Variant.Primary
        enabled: root.canBid
        text: root.oracleUnknown ? qsTr("Oracle unknown")
              : root.blocked ? qsTr("Bidding paused")
              : d.bidder === null ? qsTr("Pick the signing account")
              : !root.hasStableHolding ? qsTr("No stable holding")
              : d.quoting ? qsTr("Quoting…") : qsTr("Place bid")
        onClicked: d.placeBid()
        // Primary buttons carry a black label (contrast on the orange fill), 14 px bold as on the board.
        Component.onCompleted: {
            labelItem.color = Qt.binding(function() {
                return enabled ? Theme.palette.backgroundBlack : Theme.palette.textMuted
            })
            labelItem.font.pixelSize = Theme.typography.primaryText
            labelItem.font.weight = Theme.typography.weightBold
        }
    }

    TxLane {
        id: lane
        Layout.fillWidth: true
        visible: txId.length > 0
        backend: root.backend
        accounts: root.accounts
        onStateChanged: if (state === "done") root.laneDone()
        onRequote: d.placeBid()
    }

    TxDialog {
        id: dialog
        onConfirmed: d.submit()
    }
}
