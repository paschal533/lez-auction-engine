# Draft: status note on RFP-014 (for the user to post, or not)

**Where:** a comment on logos-co/ecosystem#205, or Discord `#builder-hub`.
**Status:** DRAFT. Not posted. Edit freely before posting. Verify the repository visibility before posting, and
adjust the "State" line to match. If the 2026-10-01 intro note was not posted, this note stands on its own as
written; it does not refer to it.

---

Status update on my RFP-014 (Liquidation & Auction Engine) proof of concept, LEZ v0.2.4 + SPEL, localnet.

Built: liquidation with a keeper reward (F1); increasing discount with early termination (F2); partial bids (F3); debt auction and the freeze-new-debt path, with a mock protocol token (F4); surplus auction with burn (F5); an SDK and CLI with their own encoders; a three-instruction stablecoin hook patch series; real-proof CI per ADR-045 (workflows written, not yet run on a hosted runner) and a one-command localnet demo.

Testnet: deployed and tested on the public legacy testnet (`testnet-legacy.lez.logos.co`, LEZ v0.2.4) on 2026-10-17; the full demo scenario completed with every check ok. Testnet 0.4 waits for the port (blocked by the 0.4 release and SPEL support).

Not built: the GUI (next), the private path (M4).

State: a feature branch in a private repository; the hosted demo and real-proof jobs have not run. The red-team pass found nothing above Low.

Open questions: is the GUI part of this RFP or separate, is a hook-gated `liquidate_position` the stablecoin shape you want, and is a SPEL release for v0.3 planned? Two upstream issue drafts and the hook PR are written, not filed.
