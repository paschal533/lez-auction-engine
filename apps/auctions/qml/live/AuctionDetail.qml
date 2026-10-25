import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format

// The panel of the Live screen (board v3): the selected auction's lot and status, the KPI strip (discount with
// its rate per minute, the time the maximum is reached, raised of target with a bar, collateral left), the
// discount schedule chart, the facts grid and the bid card. The engine has no end time (R30): the second KPI
// is "Max discount at hh:mm", or "Held at max" once the schedule is flat.
Rectangle {
    id: root
    objectName: "auctionDetail"

    property var backend: null
    // The `auction` view (live), with `schedule`.
    property var auction: null
    property var oracle: null
    property var accounts: []
    property string activeAccount: ""

    readonly property bool hasAuction: !!(auction && auction.id)
    readonly property string auctionId: hasAuction ? String(auction.id) : ""
    readonly property string leftToRaise: hasAuction ? Format.subtract(String(auction.target), String(auction.raised)) : "0"

    signal laneDone()

    function clock(ms) { return Qt.formatTime(new Date(Number(ms)), "hh:mm") }

    color: Theme.palette.surfaceRaised
    radius: Theme.spacing.radiusXlarge

    LogosText {
        objectName: "auctionEmpty"
        anchors.centerIn: parent
        visible: !root.hasAuction
        text: qsTr("No live auction selected.")
        color: Theme.palette.textTertiary
    }

    // The content scrolls when the dock is shorter than it (a bid in flight adds the lane to the bid card).
    Flickable {
        id: flick
        objectName: "auctionScroll"
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
                    objectName: "auctionLot"
                    text: root.hasAuction ? Format.amount(String(root.auction.lot), 0) : ""
                    font.pixelSize: Theme.typography.panelTitleText
                    font.weight: Theme.typography.weightMedium
                    font.features: { "tnum": 1 }
                    color: Theme.palette.text
                }
                LogosText {
                    text: qsTr("collateral")
                    font.pixelSize: Theme.typography.panelTitleText
                    color: Theme.palette.textSecondary
                }
                LogosBadge {
                    objectName: "auctionIdBadge"
                    text: root.hasAuction ? Format.shortId(root.auctionId) : ""
                    color: Theme.palette.textSecondary
                }
                LogosBadge {
                    objectName: "auctionStatus"
                    text: root.hasAuction && root.auction.paused ? qsTr("Bidding paused") : qsTr("Bidding open")
                    color: root.hasAuction && root.auction.paused ? Theme.palette.warning : Theme.palette.success
                }
                Item { Layout.fillWidth: true }
                LogosText {
                    objectName: "auctionStarted"
                    text: root.hasAuction
                          ? qsTr("Position %1 · started %2").arg(Format.shortId(String(root.auction.position)))
                                                            .arg(root.clock(root.auction.started_at_ms))
                          : ""
                    font.pixelSize: Theme.typography.secondaryText
                    font.features: { "tnum": 1 }
                    color: Theme.palette.textSecondary
                }
            }

            // ---- KPIs ---------------------------------------------------------------------------------
            KpiStrip {
                Layout.fillWidth: true
                items: !root.hasAuction ? [] : [
                    { label: qsTr("Current discount"), value: Format.rayPercent(String(root.auction.discount_ray), 1) + "%",
                      accent: true,
                      caption: root.auction.held_at_max
                               ? qsTr("At its maximum")
                               : qsTr("+%1% per min").arg(Format.rayPercent(String(root.auction.rate_per_min_ray), 2)) },
                    root.auction.held_at_max
                        ? { label: qsTr("Max discount at"), value: qsTr("Held at max"),
                            caption: qsTr("since %1").arg(root.clock(root.auction.max_at_ms)) }
                        : { label: qsTr("Max discount at"), value: root.clock(root.auction.max_at_ms),
                            caption: qsTr("then holds") },
                    { label: qsTr("Raised"), value: String(root.auction.raised), decimals: 0,
                      unit: "/ " + Format.amount(String(root.auction.target), 0) + " " + qsTr("stable"),
                      progress: Format.ratio(String(root.auction.raised), String(root.auction.target)) },
                    { label: qsTr("Collateral left"), value: String(root.auction.collateral_left), decimals: 0,
                      caption: qsTr("of %1").arg(Format.amount(String(root.auction.lot), 0)) }
                ]
            }

            // ---- Chart + facts | bid card -------------------------------------------------------------
            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: Theme.spacing.medium

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.alignment: Qt.AlignTop
                    spacing: Theme.spacing.medium

                    InsetCard {
                        Layout.fillWidth: true
                        padding: Theme.spacing.medium
                        spacing: Theme.spacing.tiny

                        RowLayout {
                            Layout.fillWidth: true
                            LogosText {
                                Layout.fillWidth: true
                                text: qsTr("Discount schedule")
                                font.pixelSize: Theme.typography.primaryText
                                font.weight: Theme.typography.weightBold
                                color: Theme.palette.text
                            }
                            LogosText {
                                objectName: "chartCaption"
                                text: !root.hasAuction ? ""
                                      : root.auction.held_at_max
                                        ? qsTr("Held at its maximum since %1").arg(root.clock(root.auction.max_at_ms))
                                        : qsTr("Reaches its maximum at %1, then holds").arg(root.clock(root.auction.max_at_ms))
                                font.pixelSize: Theme.typography.secondaryText
                                font.features: { "tnum": 1 }
                                color: Theme.palette.textSecondary
                            }
                        }

                        DiscountChart {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 190
                            schedule: root.hasAuction && root.auction.schedule ? root.auction.schedule : []
                            maxAtMs: root.hasAuction ? Number(root.auction.max_at_ms) - Number(root.auction.started_at_ms) : 0
                            maxAtClockMs: root.hasAuction ? Number(root.auction.max_at_ms) : 0
                            nowMs: root.hasAuction ? Number(root.auction.t_eff_ms) : 0
                            nowRay: root.hasAuction ? String(root.auction.discount_ray) : "0"
                        }
                    }

                    GridLayout {
                        objectName: "auctionFacts"
                        Layout.fillWidth: true
                        Layout.leftMargin: Theme.spacing.tiny
                        columns: 3
                        rowSpacing: Theme.spacing.medium
                        columnSpacing: Theme.spacing.large
                        uniformCellWidths: true

                        Fact {
                            Layout.fillWidth: true
                            label: qsTr("Lot")
                            value: root.hasAuction ? String(root.auction.lot) : ""
                            decimals: 0
                            unit: qsTr("collateral")
                        }
                        Fact {
                            Layout.fillWidth: true
                            label: qsTr("Target (debt + penalty)")
                            value: root.hasAuction ? String(root.auction.target) : ""
                            decimals: 0
                            unit: qsTr("stable")
                        }
                        Fact {
                            Layout.fillWidth: true
                            label: qsTr("Bids so far")
                            value: root.hasAuction ? String(root.auction.bids) : ""
                        }
                        Fact {
                            objectName: "factLeftToRaise"
                            Layout.fillWidth: true
                            label: qsTr("Left to raise")
                            value: root.leftToRaise
                            decimals: 0
                            unit: qsTr("stable")
                        }
                        Fact {
                            Layout.fillWidth: true
                            label: qsTr("Position")
                            value: root.hasAuction ? Format.shortId(String(root.auction.position)) : ""
                        }
                        Fact {
                            Layout.fillWidth: true
                            label: qsTr("Status")
                            value: !root.hasAuction ? ""
                                   : root.auction.paused ? qsTr("Paused (%1)").arg(String(root.auction.paused))
                                   : root.auction.held_at_max ? qsTr("Held at max") : qsTr("Auctioning")
                        }
                    }

                    Item { Layout.fillHeight: true }
                }

                BidCard {
                    Layout.preferredWidth: 300
                    Layout.alignment: Qt.AlignTop
                    backend: root.backend
                    auction: root.auction
                    oracle: root.oracle
                    accounts: root.accounts
                    activeAccount: root.activeAccount
                    onLaneDone: root.laneDone()
                }
            }
        }
    }
}
