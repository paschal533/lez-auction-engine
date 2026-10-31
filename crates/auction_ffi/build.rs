//! Regenerates `include/auction_ffi.h` from the `extern "C"` items of `src/lib.rs`.
//! The header is committed; CI (`scripts/ci/ffi-header-drift.sh`) fails when it is stale.

use std::env;
use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let header = dir.join("include").join("auction_ffi.h");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    // A deleted or edited header is regenerated on the next build.
    println!("cargo:rerun-if-changed=include/auction_ffi.h");
    match cbindgen::generate(&dir) {
        Ok(bindings) => {
            bindings.write_to_file(&header);
        }
        Err(e) => panic!("cbindgen failed: {e}"),
    }
}
