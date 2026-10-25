import QtQuick
import QtQuick.Layouts

import Logos.Theme

// An inset card inside a panel (boards v3/v4 `.card`, the RepositoriesView row look): page background,
// a borderSubtle hairline, radiusLarge. Children go into a padded column.
Rectangle {
    id: root

    default property alias content: column.data
    property int padding: Theme.spacing.large
    property alias spacing: column.spacing

    color: Theme.palette.background
    border.color: Theme.palette.borderSubtle
    border.width: 1
    radius: Theme.spacing.radiusLarge

    implicitWidth: column.implicitWidth + 2 * padding
    implicitHeight: column.implicitHeight + 2 * padding

    ColumnLayout {
        id: column
        anchors.fill: parent
        anchors.margins: root.padding
        spacing: Theme.spacing.small
    }
}
