import QtQuick

import Logos.Controls

// Shown while the node does not answer (Review Focus 1): the backend's 5 s block poll failed with a
// `chain` error. The app stays usable; figures on screen are the last ones read.
LogosNotice {
    id: root
    objectName: "nodeNotice"

    property bool reachable: true
    // Only once the wallet is open: before that, startup errors have their own notice.
    property bool active: false

    severity: LogosNotice.Warning
    title: qsTr("Node not answering")
    message: qsTr("Figures on screen may be out of date. The app checks again every 5 s; "
                  + "a transaction in flight keeps its lock until its outcome is known.")
    shown: active && !reachable
}
