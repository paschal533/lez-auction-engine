//! Builds the stablecoin guest with feature `instance-b` (distinct ImageID) for two-instance tests.
use std::collections::HashMap;

use risc0_build::{embed_methods_with_options, GuestOptionsBuilder};

fn main() {
    let options = GuestOptionsBuilder::default()
        .features(vec![String::from("instance-b")])
        .build()
        .expect("guest options");
    embed_methods_with_options(HashMap::from([("stablecoin-guest", options)]));
}
