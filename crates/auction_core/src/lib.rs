//! Pure, LEZ-independent logic for the RFP-014 liquidation & auction engine.
//! Every state transition returns a new value; nothing here performs I/O.
#![cfg_attr(not(test), no_std)] // unit tests (proptest) need std; the library itself is no_std
#![forbid(unsafe_code)]
#![deny(clippy::arithmetic_side_effects)]

#[cfg(test)]
mod smoke {
    #[test]
    fn crate_builds() {}
}
