//! C ABI of the auction engine for the Basecamp core module `auction_engine`.
//!
//! Every function takes a UTF-8 JSON request and returns a heap UTF-8 JSON envelope
//! (`envelope.rs`); the caller frees the reply with `auction_ffi_free`. The library reads the chain
//! and builds plans; it never signs: the module signs and sends through `lez_core`.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod ctx;
pub mod envelope;
pub mod prepare;
pub mod quotes;
pub mod track;
pub mod view;

use std::ffi::{c_char, CStr, CString};

use serde_json::{json, Value};

use crate::{
    ctx::{Ctx, CtxJson},
    envelope::FfiError,
};

/// The LEZ release the SDK is pinned to.
const LEZ_VERSION: &str = "v0.2.4";

/// Reads the request, runs `op` and hands the reply to the caller as a C string.
///
/// # Safety
/// `req` is null or points to a NUL-terminated string valid for the call.
unsafe fn call(
    req: *const c_char,
    op: impl FnOnce(Value) -> Result<Value, FfiError>,
) -> *mut c_char {
    let reply = if req.is_null() {
        envelope::run(None, op)
    } else {
        // SAFETY: not null; the caller guarantees a NUL-terminated string.
        match unsafe { CStr::from_ptr(req) }.to_str() {
            Ok(text) => envelope::run(Some(text), op),
            Err(_) => envelope::reject("the request is not valid UTF-8"),
        }
    };
    CString::new(reply).map_or_else(
        |_| {
            CString::from(c"{\"ok\":false,\"error\":{\"kind\":\"panic\",\"message\":\"reply contained NUL\"}}")
                .into_raw()
        },
        CString::into_raw,
    )
}

/// `{"crate":"0.1.0","sdk":"...","lez":"v0.2.4"}`.
///
/// # Safety
/// `req` is null or points to a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn auction_ffi_version(req: *const c_char) -> *mut c_char {
    // SAFETY: forwarded from the caller's guarantee.
    unsafe {
        call(req, |_| {
            Ok(json!({
                "crate": env!("CARGO_PKG_VERSION"),
                "sdk": auction_sdk::VERSION,
                "lez": LEZ_VERSION,
            }))
        })
    }
}

/// `{"view":"deployment","ctx":{...}}` -> `{network, dev, config, genesis_ok}`.
///
/// # Safety
/// `req` is null or points to a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn auction_ffi_view(req: *const c_char) -> *mut c_char {
    // SAFETY: forwarded from the caller's guarantee.
    unsafe { call(req, view::handle) }
}

/// `{"ctx":{...},"action":"token_transfer",...}` -> `{"steps":[Plan],"facts":[]}`.
///
/// # Safety
/// `req` is null or points to a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn auction_ffi_prepare(req: *const c_char) -> *mut c_char {
    // SAFETY: forwarded from the caller's guarantee.
    unsafe { call(req, |v| prepare::handle(&Ctx::from_request(&v)?, &v)) }
}

/// `{"ctx":{...},"signer":"<base58>"}` -> the `Tracking`; takes the signer's in-flight lock.
///
/// # Safety
/// `req` is null or points to a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn auction_ffi_tx_begin(req: *const c_char) -> *mut c_char {
    // SAFETY: forwarded from the caller's guarantee.
    unsafe { call(req, |v| track::begin(&Ctx::for_tx(&v)?, &v)) }
}

/// `{"ctx":{...},"tracking":{...}}` -> pending, included or dropped.
///
/// # Safety
/// `req` is null or points to a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn auction_ffi_tx_poll(req: *const c_char) -> *mut c_char {
    // SAFETY: forwarded from the caller's guarantee.
    unsafe { call(req, |v| track::poll(&Ctx::for_tx(&v)?, &v)) }
}

/// `{"ctx":{...},"tracking":{...},"outcome":"included|dropped|unknown"}` -> `{}`.
///
/// # Safety
/// `req` is null or points to a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn auction_ffi_tx_end(req: *const c_char) -> *mut c_char {
    // SAFETY: forwarded from the caller's guarantee.
    unsafe { call(req, |v| track::end(&CtxJson::of(&v)?, &v)) }
}

/// Frees a reply returned by any `auction_ffi_*` function. Null is a no-op.
///
/// # Safety
/// `s` is null or a pointer returned by this library that has not been freed yet.
#[no_mangle]
pub unsafe extern "C" fn auction_ffi_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: `s` came from `CString::into_raw` in `call` and is freed once.
        drop(unsafe { CString::from_raw(s) });
    }
}
