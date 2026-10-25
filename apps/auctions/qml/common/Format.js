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

function isDigits(s) {
    return typeof s === "string" && /^\d+$/.test(s)
}

function stripZeros(digits) {
    return digits.replace(/^0+(?=\d)/, "")
}

// cmp("700000", "660000") -> 1: compares two decimal strings (-1, 0, 1) without a double.
function cmp(a, b) {
    var x = stripZeros(String(a))
    var y = stripZeros(String(b))
    if (x.length !== y.length)
        return x.length < y.length ? -1 : 1
    return x === y ? 0 : (x < y ? -1 : 1)
}

// subtract("660000", "100000") -> "560000". Floors at "0": what is left never goes negative.
function subtract(a, b) {
    var x = stripZeros(String(a))
    var y = stripZeros(String(b))
    if (!isDigits(x) || !isDigits(y) || cmp(x, y) <= 0)
        return "0"
    var out = ""
    var borrow = 0
    for (var i = 0; i < x.length; i++) {
        var dx = x.charCodeAt(x.length - 1 - i) - 48
        var dy = i < y.length ? y.charCodeAt(y.length - 1 - i) - 48 : 0
        var d = dx - dy - borrow
        borrow = d < 0 ? 1 : 0
        out = String(d < 0 ? d + 10 : d) + out
    }
    return stripZeros(out)
}

// add("600000", "60000") -> "660000": the sum of two decimal strings, digit by digit (no double).
// Input that is not digits gives "".
function add(a, b) {
    var x = stripZeros(String(a))
    var y = stripZeros(String(b))
    if (!isDigits(x) || !isDigits(y))
        return ""
    var out = ""
    var carry = 0
    for (var i = 0; i < Math.max(x.length, y.length); i++) {
        var dx = i < x.length ? x.charCodeAt(x.length - 1 - i) - 48 : 0
        var dy = i < y.length ? y.charCodeAt(y.length - 1 - i) - 48 : 0
        var d = dx + dy + carry
        carry = d >= 10 ? 1 : 0
        out = String(d % 10) + out
    }
    return carry > 0 ? "1" + out : out
}

// fixed("12345", 2, 1) -> "123.4": the value decString / 10^decimals truncated (floored) to exactly `places`
// fraction digits, whole part grouped, as the CLI prints (cli/src/output.rs bps(), R32). Input that is not
// digits comes back unchanged.
function fixed(decString, decimals, places) {
    var s = String(decString === undefined || decString === null ? "" : decString)
    if (!isDigits(s) || !(decimals >= 0) || !(places >= 0))
        return s
    s = stripZeros(s)
    if (places >= decimals) {
        for (var i = decimals; i < places; i++)
            s += "0"
    } else {
        var drop = decimals - places
        while (s.length <= drop)
            s = "0" + s
        s = s.substring(0, s.length - drop)
    }
    while (s.length <= places)
        s = "0" + s
    var whole = group(stripZeros(s.substring(0, s.length - places)))
    return places > 0 ? whole + "." + s.substring(s.length - places) : whole
}

// Ray values carry 27 decimals (1e27 = 1, so 1e25 = 1 %).
var RAY_DECIMALS = 27
var RAY_PERCENT_DECIMALS = 25

// rayPercent("37777777777777777777777777", 1) -> "3.7" (percent, no sign, truncated like the CLI).
function rayPercent(ray, places) {
    return fixed(ray, RAY_PERCENT_DECIMALS, places)
}

// rayPrice("500000000000000000000000000") -> "0.5": a ray price truncated to 6 places, trailing zeros dropped.
function rayPrice(ray) {
    var s = fixed(ray, RAY_DECIMALS, 6)
    return s.indexOf(".") < 0 ? s : s.replace(/0+$/, "").replace(/\.$/, "")
}

// ratio("330000", "660000") -> 0.5, clamped to [0, 1]: a share for bars, from the leading digits only.
var RATIO_DIGITS = 15
function ratio(part, whole) {
    var a = stripZeros(String(part))
    var b = stripZeros(String(whole))
    if (!isDigits(a) || !isDigits(b) || b === "0")
        return 0
    var cut = Math.max(0, b.length - RATIO_DIGITS)
    var an = a.length > cut ? Number(a.substring(0, a.length - cut)) : 0
    var r = an / Number(b.substring(0, b.length - cut))
    return Math.max(0, Math.min(1, r))
}

// percentToBps("0.5") -> 50; anything that is not 0–99.99 with at most two decimals -> -1.
function percentToBps(text) {
    var m = /^(\d{1,2})(?:\.(\d{1,2}))?$/.exec(String(text))
    if (!m)
        return -1
    var frac = m[2] === undefined ? "00" : (m[2] + "0").substring(0, 2)
    return Number(m[1]) * 100 + Number(frac)
}

// mul("12", "12") -> "144": the product of two decimal strings, digit by digit (no double), for comparisons
// of returned fields such as lot × RAY against target × price. Input that is not digits gives "".
function mul(a, b) {
    var x = stripZeros(String(a))
    var y = stripZeros(String(b))
    if (!isDigits(x) || !isDigits(y))
        return ""
    var out = []
    for (var k = 0; k < x.length + y.length; k++)
        out.push(0)
    for (var i = x.length - 1; i >= 0; i--) {
        var carry = 0
        var dx = x.charCodeAt(i) - 48
        for (var j = y.length - 1; j >= 0; j--) {
            var cell = i + j + 1
            var t = out[cell] + dx * (y.charCodeAt(j) - 48) + carry
            out[cell] = t % 10
            carry = Math.floor(t / 10)
        }
        out[i] += carry
    }
    return stripZeros(out.join(""))
}

// bpsPercent(16666, 1) -> "166.6": basis points as a percent, truncated like the CLI (R32).
function bpsPercent(bps, places) {
    return fixed(String(bps), 2, places)
}

// bpsPercentTrim(20000) -> "200", bpsPercentTrim(15050) -> "150.5": a parameter in bps as a percent,
// trailing zeros dropped (exact: bps carry two decimals of a percent).
function bpsPercentTrim(bps) {
    var s = bpsPercent(bps, 2)
    return s.indexOf(".") < 0 ? s : s.replace(/0+$/, "").replace(/\.$/, "")
}

// toUnits("1.9", 27) -> "1900000000000000000000000000": a typed decimal as an integer string at `decimals`
// places (a RAY price from "1.9"), digit by digit. More fraction digits than `decimals`, or anything that is not
// a plain decimal, gives "".
function toUnits(text, decimals) {
    var m = /^(\d+)(?:\.(\d*))?$/.exec(String(text === undefined || text === null ? "" : text).trim())
    if (!m || !(decimals >= 0))
        return ""
    var frac = m[2] === undefined ? "" : m[2]
    if (frac.length > decimals)
        return ""
    while (frac.length < decimals)
        frac += "0"
    return stripZeros(m[1] + frac)
}

// duration(150000) -> "2m": an age in ms as whole seconds under a minute, else whole minutes (floored).
function duration(ms) {
    var n = Number(ms)
    if (!(n >= 0))
        return ""
    if (n < 60000)
        return Math.floor(n / 1000) + "s"
    return Math.floor(n / 60000) + "m"
}

// list(v) -> a JS Array. Replies cross the remote-objects replica, where a QVariantList can arrive as an array-like
// sequence (a `length` and indexes) or an object keyed "0", "1", ... rather than a JS Array; anything else gives [].
function list(v) {
    if (Array.isArray(v))
        return v
    if (!v || typeof v !== "object")
        return []
    if (typeof v.length === "number")
        return Array.prototype.slice.call(v)
    var keys = Object.keys(v)
    if (keys.length === 0 || !keys.every(function(k) { return /^\d+$/.test(k) }))
        return []
    keys.sort(function(a, b) { return Number(a) - Number(b) })
    return keys.map(function(k) { return v[k] })
}
