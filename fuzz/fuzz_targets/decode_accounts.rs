//! Any account bytes owned by the engine: every decoder returns a typed error or a state, never panics.
#![no_main]
use auction_sdk::{decode::Reader, testing::FixtureChain};
use lee_core::account::{Account, AccountId, Data};
use libfuzzer_sys::fuzz_target;

const ENGINE: [u32; 8] = [0xE1; 8];

fuzz_target!(|data: &[u8]| {
    let Ok(d) = Data::try_from(data.to_vec()) else {
        return;
    };
    let chain = FixtureChain::new();
    let id = AccountId::new([7; 32]);
    chain.put(
        id,
        Account {
            program_owner: ENGINE,
            data: d,
            ..Account::default()
        },
    );
    let r = Reader::new(&chain, ENGINE);
    let _ = r.config(&id);
    let _ = r.coll_config(&id);
    let _ = r.accounting(&id);
    let _ = r.auction(&id);
    let _ = r.surplus_auction(&id);
    let _ = r.debt_auction(&id);
    let _ = r.record_count(&id);
});
