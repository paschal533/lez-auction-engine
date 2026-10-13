//! Any deployment file: parsed or refused with a typed error, never a panic.
#![no_main]
use auction_sdk::deployment::Deployment;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = Deployment::from_json(text, "fuzz.json");
    }
});
