//! Any wallet storage text: `Keystore::parse` returns a typed error or a keystore, never panics.
#![no_main]
use std::path::Path;

use auction_sdk::keystore::Keystore;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        if let Err(e) = Keystore::parse(text, Path::new("fuzz-storage.json")) {
            let _ = e.to_string();
        }
    }
});
