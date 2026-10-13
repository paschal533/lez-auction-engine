//! Any scanner cursor file content: loading and reading its id sets never panics.
#![no_main]
use auction_sdk::scan::Cursor;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(c) = serde_json::from_slice::<Cursor>(data) {
        let _ = c.auction_ids();
        let _ = c.position_ids();
    }
});
