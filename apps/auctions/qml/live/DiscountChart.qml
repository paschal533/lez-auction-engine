import QtQuick
import QtQuick.Shapes

import Logos.Theme
import Logos.Controls

import "../common/Format.js" as Format

// The discount schedule of one auction (board v3 chart), drawn with QtQuick.Shapes from the `auction` view's
// `schedule` ([[t_ms since start, discount_ray], ...], quote::discount_schedule). The elapsed part is solid
// accentOrange over a wash, the part ahead dashed; a knee marks the maximum ("Max at hh:mm") and the flat line
// after it. The engine has no end time (R30), so there is no expiry marker: the x axis runs from 0 to
// max_at + 25 %. A "Now x.x%" pin sits at (t_eff, discount_ray).
Item {
    id: root
    objectName: "discountChart"

    property var schedule: []
    // Milliseconds from the start to the maximum (max_at_ms - started_at_ms), and the clock time of it.
    property real maxAtMs: 0
    property real maxAtClockMs: 0
    // Effective elapsed time and the discount now (t_eff_ms, discount_ray).
    property real nowMs: 0
    property string nowRay: "0"

    readonly property real plotLeft: 40
    readonly property real plotTop: 28
    readonly property real plotWidth: Math.max(1, width - plotLeft - 16)
    readonly property real plotHeight: Math.max(1, height - plotTop - 24)
    readonly property real xMaxMs: maxAtMs > 0 ? maxAtMs * 1.25 : d.lastT
    readonly property real yMaxPct: Math.max(2, Math.ceil(d.maxPct / 2) * 2)
    readonly property real nowPct: d.pct(nowRay)
    // The schedule mapped to the plot, clipped at xMaxMs: [{t, pct, x, y}].
    readonly property var points: d.mapped

    function xFor(t) { return plotLeft + plotWidth * Math.min(t, xMaxMs) / Math.max(1, xMaxMs) }
    function yFor(pct) { return plotTop + plotHeight * (1 - Math.min(pct, yMaxPct) / yMaxPct) }

    implicitHeight: 200

    QtObject {
        id: d

        // A ray as a Number percent, through the string formatter (the u128 itself never becomes a double).
        function pct(ray) {
            const s = Format.rayPercent(String(ray === undefined || ray === null ? "0" : ray), 4)
            const n = Number(s.replace(/,/g, ""))
            return isNaN(n) ? 0 : n
        }

        readonly property var raw: {
            const out = []
            const rows = root.schedule || []
            for (let i = 0; i < rows.length; i++)
                if (rows[i] && rows[i].length === 2)
                    out.push({ t: Number(rows[i][0]), pct: pct(rows[i][1]) })
            return out
        }
        readonly property real lastT: raw.length > 0 ? raw[raw.length - 1].t : 1
        readonly property real maxPct: {
            let m = 0
            for (let i = 0; i < raw.length; i++) m = Math.max(m, raw[i].pct)
            return Math.max(m, root.nowPct)
        }

        function at(t) {
            for (let i = 1; i < raw.length; i++) {
                if (raw[i].t >= t) {
                    const a = raw[i - 1], b = raw[i]
                    const f = b.t === a.t ? 1 : (t - a.t) / (b.t - a.t)
                    return a.pct + (b.pct - a.pct) * f
                }
            }
            return raw.length > 0 ? raw[raw.length - 1].pct : 0
        }

        readonly property var mapped: {
            const out = []
            const xMax = root.xMaxMs
            for (let i = 0; i < raw.length && raw[i].t <= xMax; i++)
                out.push({ t: raw[i].t, pct: raw[i].pct })
            if (raw.length > 0 && (out.length === 0 || out[out.length - 1].t < xMax))
                out.push({ t: xMax, pct: at(xMax) })
            for (let j = 0; j < out.length; j++) {
                out[j].x = root.xFor(out[j].t)
                out[j].y = root.yFor(out[j].pct)
            }
            return out
        }

        readonly property point nowPoint: Qt.point(root.xFor(root.nowMs), root.yFor(root.nowPct))

        readonly property var elapsedLine: {
            const out = []
            for (let i = 0; i < mapped.length && mapped[i].t < root.nowMs; i++)
                out.push(Qt.point(mapped[i].x, mapped[i].y))
            out.push(nowPoint)
            return out
        }
        readonly property var elapsedArea: {
            const out = elapsedLine.slice()
            const bottom = root.plotTop + root.plotHeight
            out.push(Qt.point(nowPoint.x, bottom))
            out.push(Qt.point(root.plotLeft, bottom))
            if (elapsedLine.length > 0) out.push(elapsedLine[0])
            return out
        }
        readonly property var aheadLine: {
            const out = [nowPoint]
            for (let i = 0; i < mapped.length; i++)
                if (mapped[i].t > root.nowMs) out.push(Qt.point(mapped[i].x, mapped[i].y))
            return out
        }

        readonly property var yTicks: {
            const out = []
            for (let v = 0; v <= root.yMaxPct; v += 2) out.push(v)
            return out
        }
        readonly property var xTicks: {
            const steps = [5, 10, 15, 30, 60, 120, 240]
            const spanMin = root.xMaxMs / 60000
            let step = steps[steps.length - 1]
            for (let i = 0; i < steps.length; i++)
                if (spanMin / steps[i] <= 5) { step = steps[i]; break }
            const out = []
            for (let m = 0; m <= spanMin + 1e-9; m += step) out.push(m)
            return out
        }

        function minutesLabel(m) {
            if (m === 0) return "0"
            const h = Math.floor(m / 60), r = Math.round(m % 60)
            if (h === 0) return r + "m"
            return r === 0 ? h + "h" : h + "h " + r + "m"
        }
    }

    // Gridlines and y labels.
    Repeater {
        model: d.yTicks
        Item {
            required property var modelData
            Rectangle {
                x: root.plotLeft
                y: root.yFor(modelData)
                width: root.plotWidth
                height: 1
                color: Theme.palette.borderSubtle
            }
            LogosText {
                x: 0
                width: root.plotLeft - Theme.spacing.small
                y: root.yFor(modelData) - height / 2
                horizontalAlignment: Text.AlignRight
                text: modelData + "%"
                font.pixelSize: Theme.typography.secondaryText
                font.features: { "tnum": 1 }
                color: Theme.palette.textTertiary
            }
        }
    }

    // x labels.
    Repeater {
        model: d.xTicks
        LogosText {
            required property var modelData
            x: root.xFor(modelData * 60000) - width / 2
            y: root.plotTop + root.plotHeight + Theme.spacing.tiny
            text: d.minutesLabel(modelData)
            font.pixelSize: Theme.typography.secondaryText
            font.features: { "tnum": 1 }
            color: Theme.palette.textTertiary
        }
    }

    Shape {
        anchors.fill: parent
        visible: root.points.length > 0

        // Elapsed area wash.
        ShapePath {
            strokeColor: "transparent"
            fillColor: Theme.colors.getColor(Theme.palette.accentOrange, 0.18)
            PathPolyline { path: d.elapsedArea }
        }
        // Max reference: the flat line the schedule holds at after the knee, across the plot.
        ShapePath {
            strokeColor: Theme.palette.textTertiary
            strokeWidth: 1
            strokeStyle: ShapePath.DashLine
            dashPattern: [1, 3]
            fillColor: "transparent"
            startX: root.plotLeft
            startY: root.yFor(d.maxPct)
            PathLine { x: root.plotLeft + root.plotWidth; y: root.yFor(d.maxPct) }
        }
        // Ahead: dashed.
        ShapePath {
            strokeColor: Theme.palette.accentOrange
            strokeWidth: 1.5
            strokeStyle: ShapePath.DashLine
            dashPattern: [3, 3]
            fillColor: "transparent"
            PathPolyline { path: d.aheadLine }
        }
        // Elapsed: solid.
        ShapePath {
            strokeColor: Theme.palette.accentOrange
            strokeWidth: 2
            fillColor: "transparent"
            joinStyle: ShapePath.RoundJoin
            PathPolyline { path: d.elapsedLine }
        }
    }

    // Now line down to the axis.
    Rectangle {
        visible: root.points.length > 0
        x: d.nowPoint.x
        y: d.nowPoint.y
        width: 1
        height: root.plotTop + root.plotHeight - d.nowPoint.y
        color: Theme.palette.accentOrange
    }

    // Knee: the maximum, then flat.
    Rectangle {
        objectName: "chartKnee"
        visible: root.points.length > 0 && root.maxAtMs > 0
        width: 8
        height: 8
        radius: 4
        x: root.xFor(root.maxAtMs) - width / 2
        y: root.yFor(d.maxPct) - height / 2
        color: Theme.palette.background
        border.color: Theme.palette.accentOrange
        border.width: 1.5
    }
    LogosText {
        objectName: "chartKneeLabel"
        visible: root.points.length > 0 && root.maxAtMs > 0
        x: Math.min(root.xFor(root.maxAtMs) - width / 2, root.width - width)
        y: root.yFor(d.maxPct) - height - Theme.spacing.tiny
        text: qsTr("Max at %1").arg(Qt.formatTime(new Date(root.maxAtClockMs), "hh:mm"))
        font.pixelSize: Theme.typography.secondaryText
        font.features: { "tnum": 1 }
        color: Theme.palette.textSecondary
    }

    // Now pin.
    Rectangle {
        objectName: "chartNowPin"
        visible: root.points.length > 0
        width: 10
        height: 10
        radius: 5
        x: d.nowPoint.x - width / 2
        y: d.nowPoint.y - height / 2
        color: Theme.palette.accentOrange
        border.color: Theme.palette.primaryBorder
        border.width: 3
    }
    Rectangle {
        id: nowTag
        visible: root.points.length > 0
        x: Math.min(d.nowPoint.x + Theme.spacing.small, root.width - width)
        y: Math.min(d.nowPoint.y + Theme.spacing.tiny, root.plotTop + root.plotHeight - height - 2)
        width: nowLabel.implicitWidth + 2 * Theme.spacing.small
        height: nowLabel.implicitHeight + Theme.spacing.tiny
        radius: Theme.spacing.radiusSmall
        color: Theme.palette.background
        border.color: Theme.palette.primaryBorder
        border.width: 1

        LogosText {
            id: nowLabel
            objectName: "chartNowLabel"
            anchors.centerIn: parent
            text: qsTr("Now %1%").arg(Format.rayPercent(root.nowRay, 1))
            font.pixelSize: Theme.typography.secondaryText
            font.weight: Theme.typography.weightBold
            font.features: { "tnum": 1 }
            color: Theme.palette.text
        }
    }
}
