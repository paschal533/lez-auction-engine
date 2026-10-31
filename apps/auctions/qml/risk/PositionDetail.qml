import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format

// The panel of the At-risk screen (board v4): the selected position's CR as the hero with the 100–250 % gauge,
// the facts (collateral, debt with the stability fee, redemption price, market price with freshness, allowed
// range), a short "When you liquidate" sequence, and the quote card. CR is the host's min-CR ratio at the
// redemption price R (R17), never an oracle CR. Debt value is nominal_debt × redemption_ray / RAY, floored to
// whole units as amounts are shown (R33); it needs the quote's R, so a position without a quote shows "—".
// Auctions have no end time (R30): the sequence says the auction settles once the target is raised or the lot
// is sold, nothing about time.
Rectangle {
    id: root
    objectName: "positionDetail"

    property var backend: null
    // The `position` view.
    property var position: null
    property var oracle: null
    // overview.block for the quote header, -1 while unknown.
    property int block: -1
    property var accounts: []
    property string activeAccount: ""

    readonly property bool hasPosition: !!(position && position.position)
    readonly property string positionId: hasPosition ? String(position.position) : ""
    readonly property bool hasCr: hasPosition && position.cr_bps !== null && position.cr_bps !== undefined
    readonly property var quote: hasPosition && position.quote ? position.quote : null
    // Collateral units: debt (stable) × R (collateral per stable), floored like every amount on screen.
    readonly property string debtValue: quote
        ? Format.fixed(Format.mul(String(position.nominal_debt), String(quote.redemption_ray)), Format.RAY_DECIMALS, 0)
        : ""

    signal laneDone()

    color: Theme.palette.surfaceRaised
    radius: Theme.spacing.radiusXlarge

    LogosText {
        objectName: "positionEmpty"
        anchors.centerIn: parent
        visible: !root.hasPosition
        text: qsTr("No position selected.")
        color: Theme.palette.textTertiary
    }

    Flickable {
        id: flick
        objectName: "positionScroll"
        anchors.fill: parent
        anchors.margins: Theme.spacing.large
        visible: root.hasPosition
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
                    objectName: "positionTitle"
                    text: qsTr("Position %1").arg(Format.shortId(root.positionId))
                    font.pixelSize: Theme.typography.panelTitleText
                    font.weight: Theme.typography.weightMedium
                    color: Theme.palette.text
                }
                LogosBadge {
                    objectName: "positionBadge"
                    visible: root.hasCr
                    text: root.hasPosition && root.position.liquidatable ? qsTr("Liquidatable") : qsTr("Healthy")
                    color: root.hasPosition && root.position.liquidatable ? Theme.palette.error : Theme.palette.textSecondary
                }
                Item { Layout.fillWidth: true }
                LogosText {
                    objectName: "positionOwner"
                    text: root.hasPosition ? qsTr("Owner %1").arg(Format.shortId(String(root.position.owner))) : ""
                    font.pixelSize: Theme.typography.secondaryText
                    font.family: Theme.typography.mono
                    color: Theme.palette.textSecondary
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: Theme.spacing.medium

                ColumnLayout {
                    Layout.fillWidth: true
                    // Takes what the quote card leaves; its content must not widen it past the panel.
                    Layout.preferredWidth: 0
                    Layout.minimumWidth: 0
                    Layout.fillHeight: true
                    Layout.alignment: Qt.AlignTop
                    spacing: Theme.spacing.medium

                    // ---- CR hero + gauge ----------------------------------------------------------------
                    InsetCard {
                        Layout.fillWidth: true
                        padding: Theme.spacing.large
                        spacing: Theme.spacing.small

                        LogosText {
                            text: qsTr("Collateralisation ratio")
                            font.pixelSize: Theme.typography.secondaryText
                            color: Theme.palette.textTertiary
                        }
                        RowLayout {
                            Layout.fillWidth: true
                            spacing: Theme.spacing.medium
                            LogosText {
                                objectName: "crValue"
                                text: root.hasCr ? Format.bpsPercent(root.position.cr_bps, 1) + "%" : "—"
                                font.pixelSize: Theme.typography.titleText
                                font.weight: Theme.typography.weightBold
                                font.features: { "tnum": 1 }
                                color: root.hasPosition && root.position.liquidatable ? Theme.palette.error : Theme.palette.text
                            }
                            LogosText {
                                objectName: "crCaption"
                                Layout.fillWidth: true
                                text: root.hasPosition
                                      ? qsTr("At the redemption price · min %1%").arg(Format.bpsPercentTrim(root.position.min_cr_bps))
                                      : ""
                                font.pixelSize: Theme.typography.primaryText
                                font.features: { "tnum": 1 }
                                color: Theme.palette.textSecondary
                                elide: Text.ElideRight
                            }
                        }
                        CrGauge {
                            Layout.fillWidth: true
                            // u64 bps: a real, so a huge CR cannot wrap an int (the gauge clamps it).
                            crBps: root.hasCr ? Number(root.position.cr_bps) : -1
                            minCrBps: root.hasPosition ? Number(root.position.min_cr_bps) : 0
                            liquidatable: root.hasPosition && !!root.position.liquidatable
                        }
                    }

                    // ---- Facts --------------------------------------------------------------------------
                    GridLayout {
                        objectName: "positionFacts"
                        Layout.fillWidth: true
                        Layout.leftMargin: Theme.spacing.tiny
                        columns: 2
                        rowSpacing: Theme.spacing.medium
                        columnSpacing: Theme.spacing.large
                        uniformCellWidths: true

                        Fact {
                            objectName: "factCollateral"
                            Layout.fillWidth: true
                            label: qsTr("Collateral")
                            value: root.hasPosition ? String(root.position.collateral) : ""
                            decimals: 0
                            unit: qsTr("collateral")
                        }
                        Fact {
                            objectName: "factDebt"
                            Layout.fillWidth: true
                            label: qsTr("Debt incl. stability fee")
                            value: root.hasPosition ? String(root.position.nominal_debt) : ""
                            decimals: 0
                            unit: qsTr("stable")
                        }
                        Fact {
                            objectName: "factDebtValue"
                            Layout.fillWidth: true
                            label: qsTr("Debt value")
                            value: root.debtValue.length > 0 ? root.debtValue : "—"
                            unit: root.debtValue.length > 0 ? qsTr("collateral at R") : ""
                        }
                        Fact {
                            objectName: "factRedemption"
                            Layout.fillWidth: true
                            label: qsTr("Redemption price")
                            value: root.quote ? Format.rayPrice(String(root.quote.redemption_ray)) : "—"
                            unit: qsTr("per 1 stable")
                        }
                        Fact {
                            objectName: "factMarket"
                            Layout.fillWidth: true
                            label: qsTr("Market price")
                            value: root.quote ? Format.rayPrice(String(root.quote.market_ray))
                                   : root.oracle ? Format.rayPrice(String(root.oracle.price_ray)) : "—"
                            unit: root.oracle
                                  ? qsTr("per 1 stable · %1 %2m ago").arg(root.oracle.fresh === false ? qsTr("stale") : qsTr("fresh"))
                                                                     .arg(Math.floor(Number(root.oracle.age_ms) / 60000))
                                  : qsTr("per 1 stable")
                            valueColor: root.oracle && root.oracle.fresh === false ? Theme.palette.warning : Theme.palette.text
                        }
                        Fact {
                            objectName: "factRange"
                            Layout.fillWidth: true
                            label: qsTr("Allowed range")
                            value: root.oracle ? Format.rayPrice(String(root.oracle.range_lo_ray)) + "–"
                                                 + Format.rayPrice(String(root.oracle.range_hi_ray)) : "—"
                        }
                    }

                    // ---- When you liquidate -------------------------------------------------------------
                    InsetCard {
                        Layout.fillWidth: true
                        visible: !!root.quote
                        padding: Theme.spacing.medium
                        spacing: Theme.spacing.small

                        LogosText {
                            text: qsTr("When you liquidate")
                            font.pixelSize: Theme.typography.primaryText
                            font.weight: Theme.typography.weightBold
                            color: Theme.palette.text
                        }
                        LogosText {
                            objectName: "liquidateSteps"
                            Layout.fillWidth: true
                            wrapMode: Text.WordWrap
                            lineHeight: 1.3
                            text: !root.quote ? "" : [
                                qsTr("• All %1 collateral is seized. Your reward, %2, is paid from it straight away.")
                                    .arg(Format.amount(String(root.quote.seized), 0))
                                    .arg(Format.amount(String(root.quote.keeper_reward), 0)),
                                qsTr("• Auction %1 opens for the other %2 collateral, at a discount that climbs to its maximum and then holds.")
                                    .arg(Format.shortId(String(root.quote.auction)))
                                    .arg(Format.amount(String(root.quote.lot), 0)),
                                qsTr("• It settles once %1 stable is raised or the lot is sold. It appears under Live auctions after a rescan.")
                                    .arg(Format.amount(String(root.quote.amount_to_raise), 0))
                            ].join("\n")
                            font.pixelSize: Theme.typography.secondaryText
                            font.features: { "tnum": 1 }
                            color: Theme.palette.textSecondary
                        }
                    }

                    Item { Layout.fillHeight: true }
                }

                LiquidationQuote {
                    Layout.preferredWidth: 300
                    Layout.alignment: Qt.AlignTop
                    backend: root.backend
                    position: root.position
                    oracle: root.oracle
                    block: root.block
                    accounts: root.accounts
                    activeAccount: root.activeAccount
                    onLaneDone: root.laneDone()
                }
            }
        }
    }
}
