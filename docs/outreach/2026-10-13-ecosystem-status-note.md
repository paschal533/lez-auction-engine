# Draft: status note on RFP-014 (for the user to post, or not)

**Where:** a comment on logos-co/ecosystem#205, or Discord `#builder-hub`.
**Status:** DRAFT. Not posted. Edit freely before posting, and check the repository-visibility line.

---

Status update on my RFP-014 (Liquidation & Auction Engine) proof of concept, LEZ v0.2.4 + SPEL, localnet.

Built: liquidation with a keeper reward (F1); increasing discount with early termination (F2); partial bids (F3); debt auction and the freeze-new-debt path, with a mock protocol token (F4); surplus auction with burn (F5); an SDK and CLI with their own encoders; a three-instruction stablecoin hook patch series; real-proof CI per ADR-045 and a one-command localnet demo.

Not built: the GUI (next), the private path (M4), testnet (blocked by v0.3 and SPEL support).

State: the work is on a feature branch in a private repository. The hosted CI jobs for the demo and the real proofs have not run yet. The red-team pass found nothing above Low.

Questions in my earlier note are still open. Two upstream issue drafts (spel IDL order, oracle scale) and the hook PR are written and unfiled.

<author>
