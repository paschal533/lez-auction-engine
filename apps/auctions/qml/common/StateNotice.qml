import QtQuick

import Logos.Controls

// An engine failure `{error, kind}` shown as a LogosNotice; hidden while `reply` carries no error.
// `chain` (node or RPC trouble, may pass) is a Warning; every other kind (`refused`, `budget`, `send`, ...)
// is an Error. `prefix` names what failed ("Accounts"), so the message reads "Accounts: <error>".
LogosNotice {
    id: root

    property var reply: null
    property string prefix: ""

    readonly property bool failed: !!(reply && reply.error)
    readonly property string kind: failed && reply.kind ? String(reply.kind) : ""

    severity: kind === "chain" ? LogosNotice.Warning : LogosNotice.Error
    message: failed ? (prefix.length > 0 ? prefix + ": " : "") + String(reply.error) : ""
    shown: failed
}
