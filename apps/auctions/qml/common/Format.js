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
function amount(decString, decimals) {
    var s = String(decString === undefined || decString === null ? "" : decString)
    if (s.length === 0)
        return ""
    s = s.replace(/^0+(?=\d)/, "")
    var places = decimals > 0 ? decimals : 0
    if (places === 0)
        return group(s)
    while (s.length <= places)
        s = "0" + s
    var whole = s.substring(0, s.length - places)
    var frac = s.substring(s.length - places).replace(/0+$/, "")
    return frac.length > 0 ? group(whole) + "." + frac : group(whole)
}
