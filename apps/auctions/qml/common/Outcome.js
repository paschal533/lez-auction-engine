.pragma library
.import "Format.js" as Format

// How a settled auction ended, from its `settled` / `auction` view fields (auction_core apply_bid: a bid ends
// the auction when the whole target is raised or the lot is empty). Anything else, which an engine does not
// produce, reads "Settled".
function endedHow(a) {
    if (!a) return ""
    if (Format.cmp(String(a.raised), String(a.target)) >= 0) return qsTr("Target raised")
    if (Format.cmp(String(a.sold), String(a.lot)) >= 0) return qsTr("Sold out")
    return qsTr("Settled")
}
