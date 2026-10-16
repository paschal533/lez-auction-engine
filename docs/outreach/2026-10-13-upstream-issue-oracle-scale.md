# Draft issue (for logos-blockchain/lez-programs)

**Status:** DRAFT. Not filed. The author files it, edited as they see fit.

---

**Title:** One `OraclePriceAccount`, two scales: twap_oracle writes Q64.64, the stablecoin reads 1e27

**Body**

`twap_oracle`'s `PublishPrice` stores a TWAP tick converted to a Q64.64 ratio (`price / 2^64`), and its
`CreateOraclePriceAccount` documents `initial_price` as Q64.64. The stablecoin reads
`OraclePriceAccount.price` as a 1e27 fixed-point value (its `FIXED_POINT_ONE`). lez-programs#401 appears to
discuss this. Consumers of the same struct cannot tell the scale from the account.

Consequences we hit while building an RFP-014 liquidation engine on v0.2.4:
- a live twap feed cannot drive the stablecoin (or a liquidation engine on 1e27) without a conversion and a
  convention for which scale an account carries;
- our engine therefore carries `oracle_scale` (`Ray` or `Q64x64`) per collateral and converts, but the localnet
  feed is a RAY price account created directly and rotated by our publisher (the twap path is not usable live
  yet; ADR-050 in our design log);
- `confidence_interval` is written as 0 by the producers we read (twap_oracle's `PublishPrice` path and the
  stablecoin's test setup), so a confidence check never triggers against them.

Questions: which scale should `OraclePriceAccount.price` carry? Should the account carry its scale (or a
version)? Would a "price in 1e27" producer path in twap_oracle be accepted?

<author>
