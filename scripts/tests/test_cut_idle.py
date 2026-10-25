"""Plan 4 Task 18: cut-idle.py turns story-timeline.json into cuts and title-card times."""
import importlib.util
import pathlib
import unittest

SPEC = importlib.util.spec_from_file_location(
    "cut_idle", pathlib.Path(__file__).resolve().parents[1] / "record" / "cut-idle.py")
ci = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci)


def step(sid, start_ms, end_ms, lane_wait_ms=None):
    s = {"id": sid, "start_ms": start_ms, "end_ms": end_ms}
    if lane_wait_ms is not None:
        s["lane_wait_ms"] = lane_wait_ms
    return s


class MergeTest(unittest.TestCase):
    def test_overlapping_and_touching_ranges_merge(self):
        self.assertEqual(ci.merge([(5, 8), (1, 3), (2, 4), (8, 9)]), [(1, 4), (5, 9)])

    def test_empty_and_inverted_ranges_drop(self):
        self.assertEqual(ci.merge([(3, 3), (5, 4), (1, 2)]), [(1, 2)])


class IdleCutsTest(unittest.TestCase):
    def test_long_wait_keeps_head_and_tail(self):
        # A 10 s wait starting 20 s into the story, which starts 4 s into the capture.
        cuts = ci.idle_cuts([step("a-included", 20000, 30000, 10000)], offset=4.0, threshold=3.0, keep=1.5)
        self.assertEqual(cuts, [(25.5, 32.5)])

    def test_wait_longer_than_its_step_is_clamped_to_the_step(self):
        cuts = ci.idle_cuts([step("a-included", 0, 10000, 20000)], offset=0.0, threshold=3.0, keep=1.5)
        self.assertEqual(cuts, [(1.5, 8.5)])

    def test_wait_at_or_under_threshold_is_kept(self):
        steps = [step("a", 0, 3000, 3000), step("b", 5000, 7000, 2000), step("c", 8000, 20000)]
        self.assertEqual(ci.idle_cuts(steps, offset=0.0, threshold=3.0, keep=1.5), [])


class OutTimeTest(unittest.TestCase):
    def test_times_shift_by_the_cuts_before_them(self):
        cuts = [(2.0, 5.0), (10.0, 12.0)]
        self.assertEqual(ci.out_time(1.0, cuts), 1.0)
        self.assertEqual(ci.out_time(6.0, cuts), 3.0)
        self.assertEqual(ci.out_time(13.0, cuts), 8.0)

    def test_time_inside_a_cut_maps_to_the_cut_start(self):
        self.assertEqual(ci.out_time(3.0, [(2.0, 5.0)]), 2.0)


class PlanTest(unittest.TestCase):
    def timeline(self):
        return {"steps": [
            step("f1-open", 1000, 1500),
            step("f1-included", 2000, 12000, 10000),
            step("f5-open", 13000, 13500),
            step("f5-included", 14000, 22000, 8000),
            step("freeze-open", 23000, 23500),
            step("end", 24000, 25000),
        ]}

    def test_plan_trims_head_tail_and_merges_overlaps(self):
        p = ci.plan(self.timeline(), [], offset=10.0, duration=60.0,
                    threshold=3.0, keep=1.5, lead=1.0, hold=4.0)
        # head [0, 10+1-1]; f1 wait [12+1.5, 22-1.5]; f5 wait [24+1.5, 32-1.5]; tail [35+4, 60]
        self.assertEqual(p["cuts"], [(0.0, 10.0), (13.5, 20.5), (25.5, 30.5), (39.0, 60.0)])
        self.assertAlmostEqual(p["out_duration"], 60.0 - 10.0 - 7.0 - 5.0 - 21.0)

    def test_overlapping_waits_merge_into_one_cut(self):
        tl = {"steps": [step("a", 0, 10000, 10000), step("b", 5000, 20000, 15000)]}
        p = ci.plan(tl, [], offset=0.0, duration=20.0, threshold=3.0, keep=1.5, lead=0.0, hold=0.0)
        self.assertEqual(p["cuts"], [(1.5, 18.5)])

    def test_title_cards_land_at_step_starts_after_the_cuts(self):
        cards = [("f1-open", "F1", "Liquidation"), ("f5-open", "F5", "Surplus"),
                 ("freeze-open", "Freeze", "No token")]
        p = ci.plan(self.timeline(), cards, offset=10.0, duration=60.0,
                    threshold=3.0, keep=1.5, lead=1.0, hold=4.0, card_s=3.0)
        at = [(c["id"], round(c["at"], 3), round(c["until"], 3)) for c in p["cards"]]
        # f1-open raw 11 -> 1; f5-open raw 23 -> 23-10-7 = 6; freeze-open raw 33 -> 33-10-7-5 = 11
        self.assertEqual(at, [("f1-open", 1.0, 4.0), ("f5-open", 6.0, 9.0), ("freeze-open", 11.0, 14.0)])

    def test_negative_offset_is_clamped_to_zero(self):
        # The window appeared after the story's t0: treat the capture as starting at t0.
        p = ci.plan(self.timeline(), [("f1-open", "F1", "x")], offset=-2.5, duration=40.0,
                    threshold=3.0, keep=1.5, lead=1.0, hold=4.0)
        self.assertEqual(p["offset"], 0.0)
        self.assertEqual(p["cuts"][0], (3.5, 10.5))
        self.assertAlmostEqual(p["cards"][0]["at"], 1.0)

    def test_tail_cut_starts_where_the_window_was_gone(self):
        # Basecamp closed 36 s into the capture: nothing after that is kept, even inside the hold.
        p = ci.plan(self.timeline(), [], offset=10.0, duration=60.0,
                    threshold=3.0, keep=1.5, lead=1.0, hold=4.0, end_at=36.0)
        self.assertEqual(p["cuts"][-1], (36.0, 60.0))

    def test_card_for_a_missing_step_is_an_error(self):
        with self.assertRaises(SystemExit):
            ci.plan(self.timeline(), [("nope", "X", "y")], offset=0.0, duration=60.0)


class CardsTest(unittest.TestCase):
    def test_cards_file_skips_comments_and_blank_lines(self):
        text = "# id | title | subtitle\n\nf1-open | F1 · Liquidation | Anyone can liquidate\n"
        self.assertEqual(ci.load_cards(text), [("f1-open", "F1 · Liquidation", "Anyone can liquidate")])

    def test_malformed_card_line_is_an_error(self):
        with self.assertRaises(SystemExit):
            ci.load_cards("f1-open | only a title\n")


class FilterTest(unittest.TestCase):
    def test_filter_selects_kept_frames_and_draws_each_card(self):
        p = {"cuts": [(0.0, 2.0), (5.0, 7.5)], "out_duration": 10.0,
             "cards": [{"id": "f1", "title": "F1", "subtitle": "s", "at": 1.0, "until": 4.0}]}
        f = ci.build_filter(p, fps=30, font="C:/Windows/Fonts/segoeui.ttf", text_files=["c0t.txt", "c0s.txt"])
        self.assertIn("select='not(between(t,0.000,2.000)+between(t,5.000,7.500))'", f)
        self.assertIn("setpts=N/30/TB", f)
        self.assertIn("textfile=c0t.txt", f)
        self.assertIn("enable='between(t,1.000,4.000)'", f)
        self.assertIn("fontfile='C\\:/Windows/Fonts/segoeui.ttf'", f)


class PlanOutTest(unittest.TestCase):
    def test_plan_is_written_as_utf8_json(self):
        import json
        import tempfile
        p = {"cuts": [(0.0, 1.0)], "cards": [{"id": "f1", "title": "F1 · Liquidation", "subtitle": "s",
                                              "at": 0.0, "until": 3.5}], "out_duration": 2.0}
        with tempfile.TemporaryDirectory() as d:
            path = pathlib.Path(d, "plan.json")
            ci.write_plan(p, path)
            self.assertEqual(json.loads(path.read_text(encoding="utf-8"))["cards"][0]["title"], "F1 · Liquidation")


if __name__ == "__main__":
    unittest.main()
