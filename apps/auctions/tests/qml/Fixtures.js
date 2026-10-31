.pragma library

// Reads one of the generated fixtures (scripts/basecamp/gen-qml-fixtures.sh): the JSON the auction_engine
// module returns to QML for the standard fixture. qml-tests.sh sets QML_XHR_ALLOW_FILE_READ for this.
function load(name) {
    var xhr = new XMLHttpRequest()
    xhr.open("GET", Qt.resolvedUrl("fixtures/" + name + ".json"), false)
    xhr.send()
    if (!xhr.responseText)
        throw new Error("fixture " + name + " is empty or unreadable")
    return JSON.parse(xhr.responseText)
}
