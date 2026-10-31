import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format
import "../common/Outcome.js" as Outcome

// The panel of the Settled screen (board v5): title with how the auction ended, the KPI strip (raised of
// target, collateral sold of the lot, bids), "Where the N went", the facts and the claim card. Every figure is
// a field of the `auction` view (SettledAuctionView, what `auction-cli auctions` prints). The chain records no
// end time and the view no last discount, so the board's "Ran for" is "Bids" (R35) and "Last discount" is left
// out.
Rectangle {
    id: root
    objectName: "settledDetail"

    property var backend: null
    // The `auction` view of a settled auction.
    property var auction: null
    property var accounts: []
    property string activeAccount: ""

    readonly property bool hasAuction: !!(auction && auction.id)
    readonly property string auctionId: hasAuction ? String(auction.id) : ""
    readonly property string endedHow: hasAuction ? Outcome.endedHow(auction) : ""
    readonly property string ownerName: {
        if (!hasAuction) return ""
        const rows = accounts || []
        for (let i = 0; i < rows.length; i++)
            if (rows[i] && !rows[i].error && String(rows[i].id) === String(auction.owner)) return String(rows[i].label)
        return Format.shortId(String(auction.owner))
    }

    signal laneDone()
    signal systemRequested(string section)
    signal auctionRead(var reply)

    color: Theme.palette.surfaceRaised
    radius: Theme.spacing.radiusXlarge

    LogosText {
        objectName: "settledEmpty"
        anchors.centerIn: parent
        visible: !root.hasAuction
        text: qsTr("No settled auction selected.")
        color: Theme.palette.textTertiary
    }

    Flickable {
        id: flick
        objectName: "settledScroll"
        anchors.fill: parent
        anchors.margins: Theme.spacing.large
        visible: root.hasAuction
        clip: true
        contentWidth: width
        contentHeight: Math.max(height, content.implicitHeight)
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: LogosScrollBar { policy: flick.contentHeight > flick.height ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff }

        ColumnLayout {
            id: content
            width: flick.width
            height: flick.contentHeight
            spacing: Theme.spacing.medium

            // ---- Title --------------------------------------------------------------------------------
            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.spacing.medium

                LogosText {
                    objectName: "settledTitle"
                    text: qsTr("Auction %1").arg(Format.shortId(root.auctionId))
                    font.pixelSize: Theme.typography.panelTitleText
                    font.weight: Theme.typography.weightMedium
                    color: Theme.palette.text
                }
                LogosBadge {
                    objectName: "settledBadge"
                    visible: root.hasAuction
                    text: root.endedHow
                    color: root.hasAuction && Format.cmp(String(root.auction.shortfall), "0") > 0
                           ? Theme.palette.error : Theme.palette.textSecondary
                }
                Item { Layout.fillWidth: true }
                LogosText {
                    objectName: "settledOwner"
                    text: root.hasAuction ? qsTr("Owner %1").arg(root.ownerName) : ""
                    font.pixelSize: Theme.typography.secondaryText
                    font.family: Theme.typography.mono
                    color: Theme.palette.textSecondary
                }
            }
            LogosText {
                objectName: "settledPosition"
                text: root.hasAuction ? qsTr("Position %1").arg(Format.shortId(String(root.auction.position))) : ""
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textTertiary
            }

            KpiStrip {
                objectName: "settledKpis"
                Layout.fillWidth: true
                items: !root.hasAuction ? [] : [
                    { label: qsTr("Raised"), value: String(root.auction.raised), decimals: 0,
                      caption: qsTr("of %1 stable").arg(Format.amount(String(root.auction.target), 0)),
                      progress: Format.ratio(String(root.auction.raised), String(root.auction.target)) },
                    { label: qsTr("Collateral sold"), value: String(root.auction.sold), decimals: 0,
                      caption: qsTr("of %1").arg(Format.amount(String(root.auction.lot), 0)) },
                    { label: qsTr("Bids"), value: String(root.auction.bids),
                      caption: qsTr("seen by the scan") }
                ]
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: Theme.spacing.medium

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.preferredWidth: 0
                    Layout.minimumWidth: 0
                    Layout.fillHeight: true
                    Layout.alignment: Qt.AlignTop
                    spacing: Theme.spacing.medium

                    OutcomeBar {
                        Layout.fillWidth: true
                        debtRepaid: root.hasAuction ? String(root.auction.debt_repaid) : "0"
                        penalty: root.hasAuction ? String(root.auction.penalty) : "0"
                        shortfall: root.hasAuction ? String(root.auction.shortfall) : "0"
                        raised: root.hasAuction ? String(root.auction.raised) : "0"
                    }

                    GridLayout {
                        objectName: "settledFacts"
                        Layout.fillWidth: true
                        Layout.leftMargin: Theme.spacing.tiny
                        columns: 3
                        rowSpacing: Theme.spacing.medium
                        columnSpacing: Theme.spacing.large
                        uniformCellWidths: true

                        Fact {
                            objectName: "factLot"
                            Layout.fillWidth: true
                            label: qsTr("Lot")
                            value: root.hasAuction ? String(root.auction.lot) : ""
                            decimals: 0
                            unit: qsTr("collateral")
                        }
                        Fact {
                            objectName: "factLeftover"
                            Layout.fillWidth: true
                            label: qsTr("Leftover collateral")
                            value: root.hasAuction ? String(root.auction.leftover) : ""
                            decimals: 0
                            unit: qsTr("collateral")
                        }
                        Fact {
                            objectName: "factClaimed"
                            Layout.fillWidth: true
                            label: qsTr("Claimed")
                            value: root.hasAuction ? String(root.auction.claimed) : ""
                            decimals: 0
                            unit: qsTr("collateral")
                        }
                        Fact {
                            objectName: "factTarget"
                            Layout.fillWidth: true
                            label: qsTr("Target (debt + penalty)")
                            value: root.hasAuction ? String(root.auction.target) : ""
                            decimals: 0
                            unit: qsTr("stable")
                        }
                        Fact {
                            objectName: "factRaised"
                            Layout.fillWidth: true
                            label: qsTr("Raised")
                            value: root.hasAuction ? String(root.auction.raised) : ""
                            decimals: 0
                            unit: qsTr("stable")
                        }
                        Fact {
                            objectName: "factStatus"
                            Layout.fillWidth: true
                            label: qsTr("Status")
                            value: root.hasAuction ? qsTr("Settled") : ""
                        }
                    }

                    Item { Layout.fillHeight: true }
                }

                ClaimCard {
                    Layout.preferredWidth: 300
                    Layout.alignment: Qt.AlignTop
                    backend: root.backend
                    auction: root.auction
                    accounts: root.accounts
                    activeAccount: root.activeAccount
                    onLaneDone: root.laneDone()
                    onSystemRequested: function(section) { root.systemRequested(section) }
                    onAuctionRead: function(reply) { root.auctionRead(reply) }
                }
            }
        }
    }
}
