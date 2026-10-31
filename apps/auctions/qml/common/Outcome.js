.pragma library
.import "Format.js" as Format

// How a settled auction ended, from its `settled` / `auction` view fields (auction_core/src/auction.rs):
//   Target raised   a bid raised the whole target (apply_bid);
//   Empty lot       settle_empty on a lot that was empty from the start (the keeper reward took the whole
//                   seizure): nothing sold, nothing raised;
//   Sold out        a bid emptied the lot before the target was raised (apply_bid).
// Anything else, which an engine does not produce, reads "Settled".
function endedHow(a) {
    if (!a) return ""
    if (Format.cmp(String(a.raised), String(a.target)) >= 0) return qsTr("Target raised")
    if (Format.cmp(String(a.lot), "0") === 0 && Format.cmp(String(a.raised), "0") === 0) return qsTr("Empty lot")
    if (Format.cmp(String(a.sold), String(a.lot)) >= 0) return qsTr("Sold out")
    return qsTr("Settled")
}

// A confirmation names the account that signs (captured when it opened). If the header's account changed
// while it was open, nothing is sent: returns the `{error, kind}` notice, or null when it is unchanged.
function signerChanged(opened, now) {
    if (opened === now) return null
    return { error: qsTr("the signing account changed from %1 to %2 while the confirmation was open; nothing was sent")
                    .arg(opened || qsTr("none")).arg(now || qsTr("none")),
             kind: "account_changed" }
}
