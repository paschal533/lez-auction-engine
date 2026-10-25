import QtQuick

import Logos.Theme
import Logos.Controls

// A System transaction button. `primary` (the act-now button of a card, 40 px) is the orange fill with a black
// 14 px bold label, as every primary button in the app; otherwise it is the board's upkeep button: LogosButton
// Secondary, compact (32 px, the design system's dense padding).
LogosButton {
    id: root

    property bool primary: false

    compact: !primary
    implicitHeight: primary ? 40 : 32
    radius: Theme.spacing.radiusLarge
    variant: primary ? LogosButton.Variant.Primary : LogosButton.Variant.Secondary

    Component.onCompleted: {
        if (!primary)
            return
        labelItem.color = Qt.binding(function() {
            return root.enabled ? Theme.palette.backgroundBlack : Theme.palette.textMuted
        })
        labelItem.font.pixelSize = Theme.typography.primaryText
        labelItem.font.weight = Theme.typography.weightBold
    }
}
