# Privacy (stub; full document in milestone M6)

The PoC submits public transactions only (spec §8). Everything an auction does is public: auction state,
liquidation events and amounts, price feeds and position health, and which account liquidated or bid.

What the design keeps possible for the private path (sub-project B, milestone M4): every user action settles
instantly; F4 and F5 are instant Dutch auctions with no escrow and no refunds; the only pull claim is the
position owner's leftover. `liquidate` is not private-capable on LEZ v0.2.4 with its current chain shape
(ADR-034); bids, settlement and system auctions have not been exercised on the private path.

Client behaviour the privacy guarantees will depend on (to be specified in M6): fresh ephemeral accounts per
operation (PRIV3, PA3), a non-skippable reshield (PRIV1), a private reshield target (PRIV2), and atomic
token-and-gas deshield (PA2). What happens if a user bypasses the expected path is part of the M6 document
(template S9).
