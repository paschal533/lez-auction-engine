"""The RFP-014 / λPrize watcher (scripts/rfp-watch.py) reads the two upstream READMEs correctly."""
import importlib.util
import pathlib
import unittest

SPEC = importlib.util.spec_from_file_location(
    "rfp_watch", pathlib.Path(__file__).resolve().parents[1] / "rfp-watch.py")
rw = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(rw)

HEADER = ("| ID      | Title | Tier | Status | Submission Deadline | Category | Submit Proposal |\n"
          "| ------- | ----- | ---- | ------ | ------------------- | -------- | --------------- |\n")
CLOSED = ("| RFP-014 | [Liquidation & Auction Engine](RFPs/RFP-014-liquidation-auction-engine.md) "
          "| L | closed | - | Applications & Integrations | Reopening soon |\n")
OPEN = ("| RFP-014 | [Liquidation & Auction Engine](RFPs/RFP-014-liquidation-auction-engine.md) "
        "| L | open | 30/11/26 | Applications & Integrations | [Submit](https://github.com/x) |\n")
OTHER = "| RFP-012 | [Curated Lending Vaults](RFPs/x.md) | L | closed | - | Apps | Reopening soon |\n"

PRIZES = ("| File | Description | Size | Status |\n|------|------|------|------|\n"
          "| [LP-0001](prizes/LP-0001.md) | NFT Wallet Support and Private Ownership Proof | Medium | Open |\n")


class RfpTest(unittest.TestCase):
    def test_still_reopening_soon_is_quiet(self):
        self.assertEqual(rw.rfp_findings(HEADER + OTHER + CLOSED), [])

    def test_reopened_row_is_reported_with_its_status(self):
        findings = rw.rfp_findings(HEADER + OTHER + OPEN)
        self.assertEqual(len(findings), 1)
        self.assertIn("RFP-014 changed", findings[0])
        self.assertIn("30/11/26", findings[0])

    def test_missing_row_is_reported_so_a_format_change_is_not_silent(self):
        findings = rw.rfp_findings(HEADER + OTHER)
        self.assertEqual(len(findings), 1)
        self.assertIn("not found", findings[0])

    def test_other_rfps_mentioning_reopening_do_not_mask_rfp_014(self):
        # RFP-012 still says "Reopening soon"; only the RFP-014 row decides.
        self.assertEqual(len(rw.rfp_findings(HEADER + OTHER + OPEN)), 1)


class PrizeTest(unittest.TestCase):
    def test_unrelated_prizes_are_quiet(self):
        self.assertEqual(rw.prize_findings(PRIZES), [])

    def test_auction_prize_is_reported(self):
        row = "| [LP-0030](prizes/LP-0030.md) | Dutch Auction Engine | Large | Open |\n"
        findings = rw.prize_findings(PRIZES + row)
        self.assertEqual(len(findings), 1)
        self.assertIn("LP-0030", findings[0])

    def test_liquidation_prize_is_reported_case_insensitively(self):
        row = "| [LP-0031](prizes/LP-0031.md) | LIQUIDATION keeper | Medium | Draft |\n"
        self.assertEqual(len(rw.prize_findings(PRIZES + row)), 1)

    def test_prose_mentioning_auction_outside_the_table_is_ignored(self):
        self.assertEqual(rw.prize_findings(PRIZES + "\nWe like auctions.\n"), [])


class IssueBodyTest(unittest.TestCase):
    def test_body_lists_every_finding_and_both_sources(self):
        body = rw.issue_body(["a", "b"])
        self.assertIn("- a", body)
        self.assertIn("- b", body)
        self.assertIn(rw.RFP_URL, body)
        self.assertIn(rw.PRIZE_URL, body)


if __name__ == "__main__":
    unittest.main()
