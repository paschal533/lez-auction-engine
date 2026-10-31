import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../common"
import "../common/Format.js" as Format

// Dev tools > Oracle price (board v5 "Publish oracle price"): moves the collateral's market price the way
// `auction-cli dev oracle-publish` does (ADR-050): a new price account signed by oracle-src, the host's oracle
// rotation signed by the active account (the host admin), the engine's collateral config signed by
// engine-admin. Three transactions, so the lane shows three stages (R27).
// KPIs are overview.oracle's (price, age, allowed range) and the positions quote_price returns. Prices are RAY
// (1e27 = 1), collateral per 1 stable.
// R17/R19: a position's CR is judged at the redemption price R, never the market price, and none of the three
// transactions writes R. So "What a new price does" says CR does not move, and the "At <price>" list is
// quote_price's positions exactly as returned (cr_bps_after equals cr_bps_now today); QML scales nothing.
// The price field (`devPrice`) turns red outside the allowed range and the button becomes the secondary
// "Publish anyway"; the engine still checks the price on its own feed path and refuses before signing, which
// shows on the page. "Re-publish current" (`devPriceRepublish`) is the keeper refresh: the same price again.
Item {
    id: root
    objectName: "oraclePriceDetail"

    // The DevPage: act(spec), busy, active, signer, activeAccount, backend.
    property var page: null
    property var overview: null

    readonly property var oracle: overview && overview.oracle ? overview.oracle : null
    readonly property var host: overview && overview.host ? overview.host : null
    // The typed price as RAY, or "" while it is not a price.
    readonly property string typedRay: Format.toUnits(priceField.text, Format.RAY_DECIMALS)
    readonly property bool hasPrice: typedRay.length > 0 && typedRay !== "0"
    readonly property bool inRange: !hasPrice || !oracle
        || (Format.cmp(typedRay, String(oracle.range_lo_ray)) >= 0 && Format.cmp(typedRay, String(oracle.range_hi_ray)) <= 0)
    readonly property string rangeText: oracle
        ? Format.rayPrice(String(oracle.range_lo_ray)) + "–" + Format.rayPrice(String(oracle.range_hi_ray)) : ""
    // The price quote_price is asked about: the typed one, else the current one.
    readonly property string quotedRay: hasPrice ? typedRay : (oracle ? String(oracle.price_ray) : "")
    readonly property var quote: d.quote
    readonly property var positions: quote ? Format.list(quote.positions) : []
    // The host's min CR in bps, the line CR is judged against: min_cr_ray / 1e23 rounded up, as the SDK's
    // min_cr_bps (mul_div_ceil(min_cr_ray, 10000, RAY)).
    readonly property int minCrBps: {
        if (!host || !host.min_cr_ray || !Format.isDigits(String(host.min_cr_ray))) return 0
        const s = Format.stripZeros(String(host.min_cr_ray))
        const whole = s.length > 23 ? Number(s.substring(0, s.length - 23)) : 0
        const rest = s.length > 23 ? s.substring(s.length - 23) : s
        return /[1-9]/.test(rest) ? whole + 1 : whole
    }
    readonly property int liquidatableNow: positions.filter(function(p) { return p.cr_bps_now < minCrBps }).length
    readonly property int newlyLiquidatable: positions.filter(function(p) {
        return p.cr_bps_now >= minCrBps && p.cr_bps_after < minCrBps
    }).length
    readonly property string hostAdmin: page ? page.activeAccount : ""
    readonly property bool canPublish: !!page && page.active && !page.busy && !!page.signer && !!oracle
    readonly property bool ageNearMax: !!oracle && Number(oracle.max_age_ms) > 0
                                       && Number(oracle.age_ms) * 4 >= Number(oracle.max_age_ms) * 3

    function signerText() {
        return qsTr("oracle-src, %1, engine-admin").arg(hostAdmin.length > 0 ? hostAdmin : qsTr("(no account)"))
    }

    function publish(ray) {
        if (!canPublish || !ray)
            return
        const admin = hostAdmin
        const from = page.signer ? String(page.signer.id) : ""
        const before = oracle ? Format.rayPrice(String(oracle.price_ray)) : ""
        page.act({
            title: qsTr("Publish price"),
            read: { view: "quote_price", price: ray },
            stepLabels: [
                { label: qsTr("Price account"), busyLabel: qsTr("Creating the price account…") },
                { label: qsTr("Host rotation"), busyLabel: qsTr("Host rotating the oracle…") },
                { label: qsTr("Engine follows"), busyLabel: qsTr("Engine following…") }
            ],
            dialog: function(q) {
                return {
                    action: "oracle_publish",
                    title: qsTr("Publish price"),
                    from: from,
                    facts: [
                        { label: qsTr("Price"), value: before + " → " + Format.rayPrice(String(q.price)), strong: true },
                        { label: qsTr("Engine check"), value: q.valid ? qsTr("Accepted") : qsTr("Refused: %1").arg(String(q.reason)) },
                        { label: qsTr("Signed by"), value: qsTr("oracle-src, %1, engine-admin").arg(admin) },
                        { label: qsTr("Transactions"), value: "3" }
                    ],
                    note: qsTr("A new price account (oracle-src), the host's oracle rotation (%1) and the engine's collateral config following it (engine-admin). CR is judged at the redemption price, which none of them writes.").arg(admin),
                    confirmLabel: qsTr("Publish price")
                }
            },
            submit: function(q) {
                return {
                    action: "oracle_publish",
                    price_source: "oracle-src",
                    price: String(q.price),
                    host_admin: admin,
                    engine_admin: "engine-admin"
                }
            }
        })
    }

    onQuotedRayChanged: quoteTimer.restart()
    onPageChanged: quoteTimer.restart()

    QtObject {
        id: d

        property var quote: null
        property var notice: null
        property int generation: 0

        function read() {
            const ray = root.quotedRay
            if (!root.page || !root.page.active || !root.page.backend || ray.length === 0)
                return
            const gen = ++generation
            logos.watch(root.page.backend.view({ view: "quote_price", price: ray }), function(reply) {
                if (gen !== generation) return
                if (!reply || reply.error) {
                    notice = reply && reply.error ? reply : { error: qsTr("The engine sent no quote."), kind: "bad_reply" }
                    quote = null
                    return
                }
                notice = null
                quote = reply
            }, function(err) {
                if (gen === generation) notice = { error: String(err), kind: "ipc" }
            })
        }
    }

    // Typing settles before the quote is read (one read per pause, never a poll).
    Timer {
        id: quoteTimer
        objectName: "devPriceQuoteTimer"
        interval: 300
        onTriggered: d.read()
    }

    Flickable {
        id: flick
        anchors.fill: parent
        clip: true
        contentWidth: width
        contentHeight: Math.max(height, content.implicitHeight)
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: LogosScrollBar { policy: flick.contentHeight > flick.height ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff }

        ColumnLayout {
            id: content
            width: flick.width
            spacing: Theme.spacing.medium

            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.spacing.medium
                LogosText {
                    objectName: "devPriceTitle"
                    text: qsTr("Publish oracle price")
                    font.pixelSize: Theme.typography.panelTitleText
                    font.weight: Theme.typography.weightMedium
                    color: Theme.palette.text
                }
                LogosBadge {
                    text: qsTr("Dev · oracle-src")
                    color: Theme.palette.textSecondary
                }
                Item { Layout.fillWidth: true }
                LogosText {
                    objectName: "devPriceOracle"
                    text: root.host ? qsTr("twap_oracle · %1").arg(Format.shortId(String(root.host.market_oracle))) : ""
                    font.pixelSize: Theme.typography.secondaryText
                    color: Theme.palette.textTertiary
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: Theme.palette.borderSubtle
            }

            LogosNotice {
                objectName: "devPriceAgeNotice"
                Layout.fillWidth: true
                severity: LogosNotice.Warning
                message: root.oracle
                         ? qsTr("Price is %1 old. It pauses at %2. Re-publish the same price to keep the demo running.")
                               .arg(Format.duration(root.oracle.age_ms)).arg(Format.duration(root.oracle.max_age_ms))
                         : ""
                shown: root.ageNearMax
            }

            KpiStrip {
                objectName: "devPriceKpis"
                Layout.fillWidth: true
                items: [
                    { label: qsTr("Current price"), value: root.oracle ? Format.rayPrice(String(root.oracle.price_ray)) : "—",
                      caption: qsTr("collateral per 1 stable") },
                    { label: qsTr("Age"), value: root.oracle ? Format.duration(root.oracle.age_ms) : "—",
                      caption: root.oracle ? qsTr("max %1").arg(Format.duration(root.oracle.max_age_ms)) : "" },
                    { label: qsTr("Allowed range"), value: root.rangeText || "—", caption: qsTr("½ to 2 × redemption") },
                    { label: qsTr("Positions"), value: root.quote ? String(root.positions.length) : "—",
                      caption: root.quote ? qsTr("%1 liquidatable now").arg(root.liquidatableNow) : "" }
                ]
            }

            StateNotice {
                objectName: "devPriceQuoteNotice"
                Layout.fillWidth: true
                prefix: qsTr("Price quote")
                reply: d.notice
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.spacing.medium

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.preferredWidth: 3
                    Layout.alignment: Qt.AlignTop
                    spacing: Theme.spacing.medium

                    InsetCard {
                        objectName: "devPriceEffects"
                        Layout.fillWidth: true
                        spacing: Theme.spacing.small

                        LogosText {
                            text: qsTr("What a new price does")
                            font.pixelSize: Theme.typography.primaryText
                            font.weight: Theme.typography.weightBold
                            color: Theme.palette.text
                        }
                        Repeater {
                            model: [
                                qsTr("Live auctions and new liquidations value collateral at the new price straight away: at a higher price a lot buys back less stable, so an auction can end short of its target."),
                                "",
                                qsTr("Outside the allowed range the engine refuses it before anything is signed: such a feed would pause the collateral, and there is no fallback price.")
                            ]
                            RowLayout {
                                id: effect
                                required property var modelData
                                required property int index
                                Layout.fillWidth: true
                                spacing: Theme.spacing.small
                                Rectangle {
                                    Layout.alignment: Qt.AlignTop
                                    Layout.topMargin: 6
                                    Layout.preferredWidth: 6
                                    Layout.preferredHeight: 6
                                    radius: 3
                                    color: Theme.palette.accentOrange
                                }
                                LogosText {
                                    objectName: effect.index === 1 ? "devPriceExplainer" : "devPriceEffect_" + effect.index
                                    Layout.fillWidth: true
                                    wrapMode: Text.WordWrap
                                    text: effect.index === 1
                                          ? qsTr("CR is judged at the redemption price R (%1), not the market price. Publishing writes nothing R depends on, so no position's CR changes: the list below is the engine's own reading.")
                                                .arg(root.quote ? Format.rayPrice(String(root.quote.redemption_ray)) : "…")
                                          : effect.modelData
                                    font.pixelSize: Theme.typography.secondaryText
                                    color: Theme.palette.textSecondary
                                }
                            }
                        }
                    }

                    InsetCard {
                        objectName: "devAffectedCard"
                        Layout.fillWidth: true
                        spacing: Theme.spacing.small

                        LogosText {
                            objectName: "devAffectedTitle"
                            text: root.quote ? qsTr("At %1 collateral per stable").arg(Format.rayPrice(String(root.quote.price)))
                                             : qsTr("At the new price")
                            font.pixelSize: Theme.typography.primaryText
                            font.weight: Theme.typography.weightBold
                            color: Theme.palette.text
                        }
                        LogosText {
                            objectName: "devAffectedEmpty"
                            visible: !!root.quote && root.positions.length === 0
                            text: qsTr("No scanned position has debt.")
                            font.pixelSize: Theme.typography.secondaryText
                            color: Theme.palette.textTertiary
                        }
                        Repeater {
                            objectName: "devAffectedList"
                            model: root.positions
                            RowLayout {
                                id: affected
                                required property var modelData
                                readonly property bool belowNow: modelData.cr_bps_now < root.minCrBps
                                readonly property bool belowAfter: modelData.cr_bps_after < root.minCrBps
                                objectName: "devAffected_" + modelData.position
                                Layout.fillWidth: true
                                spacing: Theme.spacing.small
                                LogosText {
                                    Layout.fillWidth: true
                                    text: Format.shortId(String(affected.modelData.position))
                                    font.pixelSize: Theme.typography.secondaryText
                                    color: Theme.palette.textSecondary
                                }
                                LogosText {
                                    text: qsTr("CR")
                                    font.pixelSize: Theme.typography.secondaryText
                                    color: Theme.palette.textTertiary
                                }
                                LogosText {
                                    objectName: "devAffectedNow"
                                    text: Format.bpsPercent(affected.modelData.cr_bps_now, 1) + "%"
                                    font.pixelSize: Theme.typography.secondaryText
                                    font.features: { "tnum": 1 }
                                    color: affected.belowNow ? Theme.palette.error : Theme.palette.text
                                }
                                LogosText {
                                    text: "→"
                                    font.pixelSize: Theme.typography.secondaryText
                                    color: Theme.palette.textTertiary
                                }
                                LogosText {
                                    objectName: "devAffectedAfter"
                                    text: Format.bpsPercent(affected.modelData.cr_bps_after, 1) + "%"
                                    font.pixelSize: Theme.typography.secondaryText
                                    font.weight: Theme.typography.weightBold
                                    font.features: { "tnum": 1 }
                                    color: affected.belowAfter ? Theme.palette.error : Theme.palette.text
                                }
                            }
                        }
                    }
                }

                InsetCard {
                    objectName: "devPriceCard"
                    Layout.fillWidth: true
                    Layout.preferredWidth: 2
                    Layout.alignment: Qt.AlignTop
                    spacing: Theme.spacing.medium

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: Theme.spacing.tiny
                        RowLayout {
                            Layout.fillWidth: true
                            LogosText {
                                Layout.fillWidth: true
                                text: qsTr("New price")
                                font.pixelSize: Theme.typography.secondaryText
                                color: Theme.palette.textTertiary
                            }
                            LogosText {
                                text: qsTr("collateral per 1 stable")
                                font.pixelSize: Theme.typography.secondaryText
                                color: Theme.palette.textTertiary
                            }
                        }
                        LogosTextField {
                            id: priceField
                            objectName: "devPrice"
                            Layout.fillWidth: true
                            Layout.preferredHeight: 44
                            placeholderText: root.oracle ? Format.rayPrice(String(root.oracle.price_ray)) : "0"
                            enabled: !!root.page && root.page.active && !root.page.busy
                            validator: RegularExpressionValidator { regularExpression: /^\d{0,12}(\.\d{0,27})?$/ }
                            Component.onCompleted: {
                                textInput.font.pixelSize = Theme.typography.subtitleText
                                textInput.font.weight = Theme.typography.weightBold
                                textInput.font.features = { "tnum": 1 }
                            }
                            // Out of range the field turns red (board v5 "Out of range").
                            Binding {
                                target: priceField.backgroundItem.border
                                property: "color"
                                value: !root.inRange ? Theme.palette.error
                                       : (priceField.textInput.activeFocus ? Theme.palette.overlayOrange : Theme.palette.backgroundElevated)
                            }
                        }
                        RowLayout {
                            Layout.fillWidth: true
                            LogosText {
                                objectName: "devPriceRange"
                                Layout.fillWidth: true
                                text: !root.hasPrice || !root.oracle ? "" : root.inRange ? qsTr("In range") : qsTr("Out of range")
                                font.pixelSize: Theme.typography.secondaryText
                                color: root.inRange ? Theme.palette.textSecondary : Theme.palette.error
                            }
                            LogosLink {
                                objectName: "devPriceRepublish"
                                text: qsTr("Re-publish current")
                                underline: false
                                enabled: root.canPublish
                                font.pixelSize: Theme.typography.secondaryText
                                onActivated: root.publish(String(root.oracle.price_ray))
                            }
                        }
                        LogosText {
                            objectName: "devPriceError"
                            Layout.fillWidth: true
                            visible: !root.inRange
                            wrapMode: Text.WordWrap
                            text: qsTr("Outside %1. The engine refuses it before signing; a feed outside the range would pause the collateral.").arg(root.rangeText)
                            font.pixelSize: Theme.typography.secondaryText
                            color: Theme.palette.error
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        LogosText {
                            text: qsTr("Becomes liquidatable")
                            font.pixelSize: Theme.typography.secondaryText
                            color: Theme.palette.textTertiary
                        }
                        RowLayout {
                            spacing: Theme.spacing.tiny
                            LogosText {
                                objectName: "devPriceNewlyLiquidatable"
                                text: root.quote ? String(root.newlyLiquidatable) : "—"
                                font.pixelSize: Theme.typography.panelTitleText
                                font.weight: Theme.typography.weightBold
                                font.features: { "tnum": 1 }
                                color: Theme.palette.text
                            }
                            LogosText {
                                text: qsTr("positions")
                                font.pixelSize: Theme.typography.primaryText
                                color: Theme.palette.textSecondary
                            }
                        }
                        LogosText {
                            objectName: "devPriceVerdict"
                            Layout.fillWidth: true
                            wrapMode: Text.WordWrap
                            visible: text.length > 0
                            text: !root.quote ? ""
                                  : root.quote.valid ? qsTr("The engine accepts this price now.")
                                  : qsTr("The engine refuses it: %1").arg(String(root.quote.reason))
                            font.pixelSize: Theme.typography.secondaryText
                            color: root.quote && !root.quote.valid ? Theme.palette.error : Theme.palette.textSecondary
                        }
                    }

                    Fact {
                        objectName: "devPriceSigners"
                        Layout.fillWidth: true
                        label: qsTr("Signed by")
                        value: root.signerText()
                    }

                    LogosButton {
                        id: publishButton
                        objectName: "devPricePublish"
                        Layout.fillWidth: true
                        implicitHeight: 40
                        radius: Theme.spacing.radiusLarge
                        text: root.inRange ? qsTr("Publish price") : qsTr("Publish anyway")
                        variant: root.inRange ? LogosButton.Variant.Primary : LogosButton.Variant.Secondary
                        enabled: root.canPublish && root.hasPrice
                        onClicked: root.publish(root.typedRay)
                        // Primary carries a black label; "Publish anyway" is the plain secondary button.
                        Component.onCompleted: labelItem.color = Qt.binding(function() {
                            if (!enabled) return Theme.palette.textMuted
                            return variant === LogosButton.Variant.Primary ? Theme.palette.backgroundBlack : Theme.palette.text
                        })
                    }
                }
            }

            Item { Layout.fillHeight: true }
        }
    }
}
