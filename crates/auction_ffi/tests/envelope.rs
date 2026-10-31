use std::ffi::{c_char, CStr, CString};

use auction_ffi::envelope::{self, FfiError};
use auction_sdk::SdkError;
use serde_json::{json, Value};

/// Reads and frees a string the library returned.
fn take(p: *mut c_char) -> Value {
    assert!(!p.is_null(), "the library never returns null");
    // SAFETY: `p` came from an `auction_ffi_*` function and is freed exactly once, below.
    let text = unsafe { CStr::from_ptr(p) }
        .to_str()
        .expect("utf-8")
        .to_owned();
    unsafe { auction_ffi::auction_ffi_free(p) };
    serde_json::from_str(&text).expect("the reply is JSON")
}

fn kind(v: &Value) -> &str {
    assert_eq!(v["ok"], false, "{v}");
    v["error"]["kind"].as_str().expect("kind")
}

#[test]
fn null_request_is_bad_request() {
    // SAFETY: a null request is part of the contract.
    let v = take(unsafe { auction_ffi::auction_ffi_version(std::ptr::null()) });
    assert_eq!(kind(&v), "bad_request");
}

#[test]
fn invalid_utf8_is_bad_request() {
    let bytes = CString::new(vec![0xff, 0xfe]).expect("no nul");
    // SAFETY: `bytes` is a NUL-terminated buffer.
    let v = take(unsafe { auction_ffi::auction_ffi_version(bytes.as_ptr()) });
    assert_eq!(kind(&v), "bad_request");
}

#[test]
fn invalid_json_is_bad_request() {
    let text = CString::new("{not json").expect("no nul");
    // SAFETY: `text` is a NUL-terminated buffer.
    let v = take(unsafe { auction_ffi::auction_ffi_prepare(text.as_ptr()) });
    assert_eq!(kind(&v), "bad_request");
}

#[test]
fn a_panic_is_caught_and_reported() {
    let reply = envelope::run(Some("{}"), |_| -> Result<Value, FfiError> {
        panic!("boom");
    });
    let v: Value = serde_json::from_str(&reply).expect("json");
    assert_eq!(kind(&v), "panic");
    assert!(v["error"]["message"].as_str().expect("m").contains("boom"));
}

#[test]
fn sdk_errors_map_to_snake_case_kinds() {
    let e = SdkError::InFlight {
        signer: "s".into(),
        lock: "l".into(),
        blocks: 3,
    };
    assert_eq!(envelope::kind_of(&e), "in_flight");
    let v = envelope::to_value(Err(FfiError::from(e)));
    assert_eq!(kind(&v), "in_flight");
    assert!(v["error"]["message"]
        .as_str()
        .expect("m")
        .contains("in flight"));
}

#[test]
fn every_sdk_error_variant_has_a_kind() {
    use auction_sdk::keystore::KeystoreError;
    let cases: Vec<(SdkError, &str)> = vec![
        (SdkError::Encode(String::new()), "encode"),
        (
            SdkError::Missing {
                what: "w",
                id: String::new(),
            },
            "missing",
        ),
        (
            SdkError::Decode {
                what: "w",
                id: String::new(),
                reason: String::new(),
            },
            "decode",
        ),
        (
            SdkError::Mismatch {
                what: "w",
                expected: String::new(),
                found: String::new(),
            },
            "mismatch",
        ),
        (
            SdkError::Engine {
                name: String::new(),
                code: 1,
            },
            "engine",
        ),
        (SdkError::HostReject(String::new()), "host_reject"),
        (SdkError::Refused(String::new()), "refused"),
        (
            SdkError::Deployment {
                path: String::new(),
                reason: String::new(),
            },
            "deployment",
        ),
        (SdkError::Chain(String::new()), "chain"),
        (SdkError::Keystore(KeystoreError::HomeNotSet), "keystore"),
        (SdkError::SendUncertain(String::new()), "send_uncertain"),
        (SdkError::Unconfirmed(String::new()), "unconfirmed"),
        (SdkError::Scan(String::new()), "scan"),
        (
            SdkError::Io {
                path: String::new(),
                reason: String::new(),
            },
            "io",
        ),
    ];
    for (e, want) in cases {
        assert_eq!(envelope::kind_of(&e), want);
    }
}

#[test]
fn version_reports_crate_sdk_and_lez() {
    let text = CString::new("{}").expect("no nul");
    // SAFETY: `text` is a NUL-terminated buffer.
    let v = take(unsafe { auction_ffi::auction_ffi_version(text.as_ptr()) });
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["value"]["crate"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["value"]["sdk"], auction_sdk::VERSION);
    assert_eq!(v["value"]["lez"], "v0.2.4");
}

#[test]
fn free_accepts_null() {
    // SAFETY: freeing null is a no-op by contract.
    unsafe { auction_ffi::auction_ffi_free(std::ptr::null_mut()) };
}

#[test]
fn an_unknown_view_is_bad_request() {
    let req = CString::new(json!({"view": "nope", "ctx": {}}).to_string()).expect("no nul");
    // SAFETY: `req` is a NUL-terminated buffer.
    let v = take(unsafe { auction_ffi::auction_ffi_view(req.as_ptr()) });
    assert_eq!(kind(&v), "bad_request");
    let message = v["error"]["message"].as_str().expect("message");
    assert!(message.contains("unknown view nope"), "{message}");
}
