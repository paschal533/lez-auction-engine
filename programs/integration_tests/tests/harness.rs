mod common;

use common::*;
use stablecoin_core::Position;

#[test]
fn world_bootstraps_the_host_and_opens_a_position() {
    let mut w = World::new();
    let pos = w.open_position(
        k::OWNER,
        k::OWNER_COLL,
        k::OWNER_STABLE,
        0,
        500_000,
        600_000,
    );
    let p: Position = w.read(pos);
    assert_eq!(
        (p.collateral_amount, p.normalized_debt_amount),
        (500_000, 600_000)
    );
    assert_eq!(w.balance(id(k::OWNER_STABLE)), 600_000);
    assert_eq!(w.balance(host_vault(pos)), 500_000);
    w.set_min_cr(2 * ONE);
}
