.pragma library

// u128 values cross every boundary as decimal strings. Nothing here may call
// Number() or parseInt() on them: a double keeps 53 bits, a u128 needs 128.

function group(digits) {
    var out = ""
    var n = digits.length
    for (var i = 0; i < n; i++) {
        if (i > 0 && (n - i) % 3 === 0)
            out += ","
        out += digits.charAt(i)
    }
    return out
}

// amount("1234567", 3) -> "1,234.567". Trailing fraction zeros are dropped.
// Input that is not digits, or a decimals that is not an integer >= 0, comes back unchanged.
function amount(decString, decimals) {
    var s = String(decString === undefined || decString === null ? "" : decString)
    if (s.length === 0)
        return ""
    // Anything that is not a plain decimal integer is shown as received, never reformatted.
    if (!/^\d+$/.test(s) || !(decimals >= 0) || Math.floor(decimals) !== decimals)
        return s
    s = s.replace(/^0+(?=\d)/, "")
    var places = decimals
    if (places === 0)
        return group(s)
    while (s.length <= places)
        s = "0" + s
    var whole = s.substring(0, s.length - places)
    var frac = s.substring(s.length - places).replace(/0+$/, "")
    return frac.length > 0 ? group(whole) + "." + frac : group(whole)
}

// shortId("7wx93ACH…ViopKg") -> "7wx9…opKg": the first and last four characters of an account id.
function shortId(id) {
    var s = String(id === undefined || id === null ? "" : id)
    return s.length <= 10 ? s : s.substring(0, 4) + "…" + s.substring(s.length - 4)
}
