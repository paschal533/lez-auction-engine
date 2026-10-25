import QtQuick
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../shell"
import "../common/Format.js" as Format

// The claim card of the Settled screen (board v5, right column). What is left to claim is the view's
// leftover − claimed (auction_core claim_leftover pays leftover_claimable − leftover_claimed). States, in order:
//   leftover to claim: Lot / Sold to bidders / You receive / To <label>-coll and `claimSubmit`, enabled only
//     when the active account's id is the auction's `owner`; otherwise `claimReason` names the owner's wallet
//     label ("Switch account to owner2") or, when the wallet does not hold it, its short id;
//   claimed: "Claimed <amount> collateral" (`claimClaimed`; the chain records no claim block, R35);
//   ended short: an Error notice (`settledShortNotice`) and `openSystemLink` → systemRequested("accounting");
//   nothing left over (`claimNothing`).
// Claiming: a fresh `auction` view (the claim's quote; it must still show something to claim) → TxDialog →
// submit {action: claim_leftover, owner, owner_holding: <label>-coll, auction} → TxLane in place of the button.
// A failed read or submit (a host refusal is kind "refused") shows in `claimNotice`.
InsetCard {
    id: root
    objectName: "claimCard"

    property var backend: null
    // The `auction` view of a settled auction (SettledAuctionView fields).
    property var auction: null
    property var accounts: []
    property string activeAccount: ""

    readonly property bool hasAuction: !!(auction && auction.id)
    readonly property string auctionId: hasAuction ? String(auction.id) : ""
    readonly property string toClaim: hasAuction ? Format.subtract(String(auction.leftover), String(auction.claimed)) : "0"
    readonly property bool claimable: Format.cmp(toClaim, "0") > 0
    readonly property bool claimed: hasAuction && !claimable && Format.cmp(String(auction.claimed), "0") > 0
    readonly property bool endedShort: hasAuction && !claimable && !claimed && Format.cmp(String(auction.shortfall), "0") > 0
    readonly property bool nothingLeft: hasAuction && !claimable && !claimed && !endedShort
    readonly property bool isOwner: d.signer !== null && hasAuction && String(d.signer.id) === String(auction.owner)
    readonly property bool laneBusy: lane.visible && lane.state === "waiting"
    readonly property bool canClaim: claimable && isOwner && !d.reading && !laneBusy

    signal laneDone()
    signal systemRequested(string section)
    // A fresh read replaced what the screen showed (claimed meanwhile, ...).
    signal auctionRead(var reply)

    padding: Theme.spacing.large
    spacing: Theme.spacing.medium

    // Another auction: its card starts clean (no notice, no lane of the previous auction's claim).
    onAuctionIdChanged: {
        d.notice = null
        lane.txId = ""
    }

    component Line: RowLayout {
        id: line
        property string label: ""
        property string value: ""
        property bool strong: false

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
            objectName: "claimRowValue"
            text: line.value
            font.pixelSize: Theme.typography.primaryText
            font.weight: line.strong ? Theme.typography.weightBold : Theme.typography.weightMedium
            font.features: { "tnum": 1 }
            color: Theme.palette.text
        }
    }

    QtObject {
        id: d

        property bool reading: false
        // The last failed read or submit, {error, kind}.
        property var notice: null

        function rowById(id) {
            const rows = root.accounts || []
            for (let i = 0; i < rows.length; i++)
                if (rows[i] && !rows[i].error && String(rows[i].id) === String(id)) return rows[i]
            return null
        }

        // The active account's wallet row (its id is what the owner check compares).
        readonly property var signer: {
            const rows = root.accounts || []
            for (let i = 0; i < rows.length; i++)
                if (rows[i] && !rows[i].error && rows[i].label === root.activeAccount) return rows[i]
            return null
        }
        readonly property var ownerRow: root.hasAuction ? rowById(root.auction.owner) : null
        readonly property string ownerName: ownerRow ? String(ownerRow.label)
                                                     : root.hasAuction ? Format.shortId(String(root.auction.owner)) : ""

        function dialogRequest(a, amount) {
            return {
                action: "claim_leftover",
                subtitle: qsTr("Auction %1").arg(Format.shortId(String(a.id))),
                from: signer ? String(signer.id) : "",
                facts: [
                    { label: qsTr("Lot"), value: String(a.lot), decimals: 0, unit: qsTr("collateral") },
                    { label: qsTr("Sold to bidders"), value: String(a.sold), decimals: 0, unit: qsTr("collateral") },
                    { label: qsTr("You receive"), value: amount, decimals: 0, unit: qsTr("collateral"), strong: true },
                    { label: qsTr("To"), value: root.activeAccount + "-coll" }
                ],
                note: qsTr("The leftover collateral of position %1 goes to %2-coll.")
                          .arg(Format.shortId(String(a.position))).arg(root.activeAccount),
                confirmLabel: qsTr("Claim leftover"),
                noun: qsTr("claim")
            }
        }

        // A fresh read of the auction, then the confirmation if there is still something to claim.
        function claim() {
            if (!root.canClaim || !root.backend)
                return
            notice = null
            if (lane.state === "done" || lane.state === "dropped")
                lane.txId = ""
            const id = root.auctionId
            reading = true
            logos.watch(root.backend.view({ view: "auction", id: id }), function(reply) {
                reading = false
                if (id !== root.auctionId) return
                if (!reply || reply.error) {
                    notice = reply && reply.error ? reply : { error: qsTr("The engine sent no reply."), kind: "bad_reply" }
                    return
                }
                root.auctionRead(reply)
                const amount = Format.subtract(String(reply.leftover), String(reply.claimed))
                if (Format.cmp(amount, "0") <= 0 || String(reply.owner) !== String(signer ? signer.id : ""))
                    return
                dialog.quote = reply
                dialog.request = dialogRequest(reply, amount)
                dialog.open()
            }, function(err) {
                reading = false
                if (id === root.auctionId) notice = { error: String(err), kind: "ipc" }
            })
        }

        function submit() {
            const a = dialog.quote
            if (!root.backend || !a)
                return
            const req = {
                action: "claim_leftover",
                owner: root.activeAccount,
                owner_holding: root.activeAccount + "-coll",
                auction: String(a.id)
            }
            logos.watch(root.backend.submit(req), function(reply) {
                if (!reply || reply.error) {
                    notice = reply && reply.error ? reply : { error: qsTr("The engine sent no reply."), kind: "bad_reply" }
                    return
                }
                lane.submitReply = reply
                lane.request = { from: signer ? String(signer.id) : "" }
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
            text: qsTr("Leftover collateral")
            font.pixelSize: Theme.typography.primaryText
            font.weight: Theme.typography.weightBold
            color: Theme.palette.text
        }
        LogosText {
            text: qsTr("owner only")
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textTertiary
        }
    }

    // ---- Leftover to claim ----------------------------------------------------------------------
    ColumnLayout {
        Layout.fillWidth: true
        visible: root.claimable
        spacing: Theme.spacing.small

        RowLayout {
            spacing: Theme.spacing.tiny
            LogosText {
                objectName: "claimAmount"
                text: Format.amount(root.toClaim, 0)
                font.pixelSize: Theme.typography.titleText
                font.weight: Theme.typography.weightBold
                font.features: { "tnum": 1 }
                color: Theme.palette.accentOrange
            }
            LogosText {
                text: qsTr("collateral")
                font.pixelSize: Theme.typography.primaryText
                color: Theme.palette.textSecondary
            }
        }
        LogosText {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            text: root.hasAuction
                  ? qsTr("The target was raised without the whole lot. What is left goes back to the owner of position %1.")
                        .arg(Format.shortId(String(root.auction.position)))
                  : ""
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textSecondary
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0
            Line {
                objectName: "claimLot"
                label: qsTr("Lot")
                value: root.hasAuction ? Format.amount(String(root.auction.lot), 0) : ""
            }
            Line {
                objectName: "claimSold"
                label: qsTr("Sold to bidders")
                value: root.hasAuction ? "− " + Format.amount(String(root.auction.sold), 0) : ""
            }
            Line {
                objectName: "claimReceive"
                label: qsTr("You receive")
                value: Format.amount(root.toClaim, 0)
                strong: true
            }
            Line {
                objectName: "claimTo"
                label: qsTr("To")
                value: root.isOwner ? root.activeAccount + "-coll" : d.ownerRow ? d.ownerRow.label + "-coll" : "—"
            }
        }
    }

    // ---- Claimed ----------------------------------------------------------------------------------
    LogosText {
        objectName: "claimClaimed"
        Layout.fillWidth: true
        visible: root.claimed
        wrapMode: Text.WordWrap
        text: root.hasAuction ? qsTr("Claimed %1 collateral").arg(Format.amount(String(root.auction.claimed), 0)) : ""
        font.pixelSize: Theme.typography.subtitleText
        font.weight: Theme.typography.weightBold
        font.features: { "tnum": 1 }
        color: Theme.palette.success
    }
    LogosText {
        Layout.fillWidth: true
        visible: root.claimed
        wrapMode: Text.WordWrap
        text: qsTr("It went to the owner's collateral holding. Nothing is left on this auction.")
        font.pixelSize: Theme.typography.secondaryText
        color: Theme.palette.textSecondary
    }

    // ---- Ended short ------------------------------------------------------------------------------
    // Board v5 "Ended short" and the detail badge: an Error.
    LogosNotice {
        objectName: "settledShortNotice"
        Layout.fillWidth: true
        severity: LogosNotice.Error
        shown: root.endedShort
        title: root.hasAuction ? qsTr("%1 stable short").arg(Format.amount(String(root.auction.shortfall), 0)) : ""
        message: root.hasAuction
                 ? qsTr("Raised %1 of %2. The %3 of debt not repaid is now unbacked debt; the buffer covers it with Settle debt on System.")
                       .arg(Format.amount(String(root.auction.raised), 0))
                       .arg(Format.amount(String(root.auction.target), 0))
                       .arg(Format.amount(String(root.auction.shortfall), 0))
                 : ""
    }
    LogosLink {
        objectName: "openSystemLink"
        visible: root.endedShort
        text: qsTr("Open System → Accounting")
        underline: false
        font.pixelSize: Theme.typography.secondaryText
        onActivated: root.systemRequested("accounting")
    }

    // ---- Nothing left over ------------------------------------------------------------------------
    LogosText {
        objectName: "claimNothing"
        Layout.fillWidth: true
        visible: root.nothingLeft
        wrapMode: Text.WordWrap
        text: qsTr("Nothing is left over on this auction, so there is nothing to claim.")
        font.pixelSize: Theme.typography.secondaryText
        color: Theme.palette.textSecondary
    }

    StateNotice {
        objectName: "claimNotice"
        Layout.fillWidth: true
        prefix: qsTr("Claim")
        reply: d.notice
    }

    LogosButton {
        objectName: "claimSubmit"
        Layout.fillWidth: true
        Layout.preferredHeight: 40
        visible: (root.claimable || root.nothingLeft) && !root.laneBusy
        radius: Theme.spacing.radiusLarge
        variant: LogosButton.Variant.Primary
        enabled: root.canClaim
        text: d.reading ? qsTr("Reading…") : qsTr("Claim leftover")
        onClicked: d.claim()
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
        objectName: "claimReason"
        Layout.fillWidth: true
        visible: root.claimable && !root.isOwner
        wrapMode: Text.WordWrap
        horizontalAlignment: Text.AlignHCenter
        text: !root.hasAuction ? ""
              : d.ownerRow
                ? qsTr("Only the owner of position %1 can claim. Switch account to %2.")
                      .arg(Format.shortId(String(root.auction.position))).arg(d.ownerName)
                : qsTr("Only the owner of position %1 can claim. Its owner %2 is not in this wallet.")
                      .arg(Format.shortId(String(root.auction.position))).arg(d.ownerName)
        font.pixelSize: Theme.typography.secondaryText
        color: Theme.palette.textSecondary
    }

    TxLane {
        id: lane
        namePrefix: "settled"
        Layout.fillWidth: true
        visible: txId.length > 0
        backend: root.backend
        accounts: root.accounts
        onStateChanged: if (state === "done") root.laneDone()
        onRequote: d.claim()
    }

    TxDialog {
        id: dialog
        namePrefix: "settled"
        onConfirmed: d.submit()
    }
}
