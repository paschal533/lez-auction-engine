#!/usr/bin/env python3
"""Cuts the idle waits out of a raw Basecamp story capture and overlays the title cards (Plan 4 Task 18).

    cut-idle.py --timeline story-timeline.json --cards title-cards.txt --in raw.mp4 --out basecamp.mp4 \
        --capture-start EPOCH_S [--skew-ms N] [--threshold 3] [--keep 1.5] [--plan-out plan.json] [--plan-only]

story.mjs logs each step's start/end (ms after its t0) and, for a tx lane step, `lane_wait_ms`: how long the lane
stayed waiting. Every wait longer than --threshold seconds is cut down to its first and last --keep seconds, so the
viewer still sees the lane go from waiting to included. The capture before the first step (Basecamp starting,
the app opening) and after the last step (plus --hold seconds, and never past --window-gone) is cut too. Overlapping cuts are merged. Each title
card is drawn for --card-s seconds from its step's start, moved earlier by the cuts before it. A lane wait longer
than its step is clamped to the step. Progress goes to stderr; the plan goes to --plan-out.

Time alignment: --capture-start is the wall-clock time (Windows clock, epoch seconds) of the first captured frame,
which ffmpeg's gdigrab prints as the input's `start:`. The timeline's `started` is the WSL clock at the story's t0;
--skew-ms is the WSL clock minus the Windows clock. --offset (seconds of capture before t0) overrides both.
"""
import argparse
import datetime
import json
import os
import pathlib
import subprocess
import sys
import tempfile


def merge(ranges):
    """Sorted, merged (start, end) ranges; empty and inverted ranges are dropped."""
    out = []
    for a, b in sorted(r for r in ranges if r[1] > r[0]):
        if out and a <= out[-1][1]:
            out[-1] = (out[-1][0], max(out[-1][1], b))
        else:
            out.append((a, b))
    return out


def idle_cuts(steps, offset, threshold=3.0, keep=1.5):
    """Capture-time ranges to cut: the middle of every lane wait longer than `threshold` seconds."""
    cuts = []
    for s in steps:
        wait = s.get("lane_wait_ms")
        if wait is None:
            continue
        # A lane wait cannot outlast its step.
        wait = min(wait, s.get("end_ms", s["start_ms"] + wait) - s["start_ms"])
        if wait / 1000.0 <= threshold:
            continue
        a = offset + s["start_ms"] / 1000.0
        cuts.append((a + keep, a + wait / 1000.0 - keep))
    return merge(cuts)


def out_time(t, cuts):
    """Where capture time t lands in the cut video (a time inside a cut lands at that cut's start)."""
    shift = 0.0
    for a, b in cuts:
        if t >= b:
            shift += b - a
        elif t > a:
            return a - shift
    return t - shift


def plan(timeline, cards, offset, duration, threshold=3.0, keep=1.5, lead=1.0, hold=4.0, card_s=3.5, end_at=None):
    """end_at: capture time by which the Basecamp window was gone; nothing after it is kept."""
    steps = timeline["steps"]
    if not steps:
        sys.exit("cut-idle: the timeline has no steps")
    # The real offset maps story time to capture time, negative included (the capture started after the story's
    # t0); only the resulting cut ranges and card times are clamped to the capture.
    first = offset + steps[0]["start_ms"] / 1000.0
    last = offset + max(s.get("end_ms", s["start_ms"]) for s in steps) / 1000.0
    tail = min(duration, last + hold)
    if end_at is not None:
        tail = min(tail, max(0.0, end_at))
    tail = max(0.0, tail)
    ranges = idle_cuts(steps, offset, threshold, keep) + [(0.0, max(0.0, first - lead)), (tail, duration)]
    cuts = merge([(max(0.0, a), min(duration, b)) for a, b in ranges])
    end = out_time(tail, cuts)
    by_id = {s["id"]: s for s in steps}
    placed = []
    for sid, title, subtitle in cards:
        if sid not in by_id:
            sys.exit(f"cut-idle: title card for step {sid}, which is not in the timeline")
        raw = min(max(0.0, offset + by_id[sid]["start_ms"] / 1000.0), tail)
        at = min(out_time(raw, cuts), end)
        placed.append({"id": sid, "title": title, "subtitle": subtitle, "at": at, "until": at + card_s})
    kept = duration - sum(b - a for a, b in cuts)
    return {"offset": offset, "cuts": cuts, "cards": placed, "out_duration": kept}


def load_cards(text):
    """`step id | title | subtitle` per line; `#` comments and blank lines are skipped."""
    cards = []
    for n, line in enumerate(text.splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = [p.strip() for p in line.split("|")]
        if len(parts) != 3 or not all(parts):
            sys.exit(f"cut-idle: title-cards line {n}: want `step id | title | subtitle`")
        cards.append(tuple(parts))
    return cards


def _font(path):
    return "'" + path.replace("\\", "/").replace(":", "\\:") + "'"


def build_filter(p, fps, font, text_files):
    """One -vf chain: constant frame rate, drop the cut frames, renumber, draw the cards.
    text_files holds two file names (title, subtitle) per card, relative to ffmpeg's working directory."""
    keep = "+".join(f"between(t,{a:.3f},{b:.3f})" for a, b in p["cuts"]) or "0"
    chain = [f"fps={fps}", f"select='not({keep})'", f"setpts=N/{fps}/TB"]
    f = _font(font)
    for i, c in enumerate(p["cards"]):
        on = f"enable='between(t,{c['at']:.3f},{c['until']:.3f})'"
        chain.append(f"drawtext=fontfile={f}:textfile={text_files[2 * i]}:fontsize=48:fontcolor=0xFF7A1A"
                     f":box=1:boxcolor=0x111111@0.9:boxborderw=22:x=(w-text_w)/2:y=h-205:{on}")
        chain.append(f"drawtext=fontfile={f}:textfile={text_files[2 * i + 1]}:fontsize=28:fontcolor=0xEDEDED"
                     f":box=1:boxcolor=0x111111@0.9:boxborderw=18:x=(w-text_w)/2:y=h-122:{on}")
    return ",".join(chain)


def write_plan(p, path):
    pathlib.Path(path).write_text(json.dumps(p, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def log(msg):
    print(msg, file=sys.stderr, flush=True)


def _started_epoch(timeline):
    iso = timeline["started"].replace("Z", "+00:00")
    return datetime.datetime.fromisoformat(iso).timestamp()


def _duration(ffprobe, path):
    out = subprocess.run([ffprobe, "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path],
                         check=True, capture_output=True, text=True).stdout
    return float(out.strip())


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--timeline", required=True)
    ap.add_argument("--cards", required=True)
    ap.add_argument("--in", dest="raw", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--capture-start", type=float, help="epoch seconds of the first captured frame")
    ap.add_argument("--skew-ms", type=float, default=0.0, help="WSL clock minus Windows clock")
    ap.add_argument("--offset", type=float, help="seconds of capture before the story's t0 (overrides)")
    ap.add_argument("--threshold", type=float, default=3.0)
    ap.add_argument("--keep", type=float, default=1.5)
    ap.add_argument("--lead", type=float, default=1.0)
    ap.add_argument("--hold", type=float, default=4.0)
    ap.add_argument("--window-gone", type=float,
                    help="epoch seconds (Windows clock) by which the Basecamp window had closed; the cut ends there")
    ap.add_argument("--card-s", type=float, default=3.5)
    ap.add_argument("--fps", type=int, default=30)
    ap.add_argument("--font", default="C:/Windows/Fonts/segoeui.ttf")
    ap.add_argument("--ffmpeg", default="ffmpeg")
    ap.add_argument("--ffprobe", default="ffprobe")
    ap.add_argument("--plan-out", help="write the plan (cuts, card times) here as UTF-8 JSON")
    ap.add_argument("--plan-only", action="store_true", help="write the plan, no video")
    a = ap.parse_args(argv)

    timeline = json.loads(pathlib.Path(a.timeline).read_text(encoding="utf-8"))
    cards = load_cards(pathlib.Path(a.cards).read_text(encoding="utf-8"))
    if a.offset is None:
        if a.capture_start is None:
            sys.exit("cut-idle: give --capture-start (or --offset)")
        a.offset = _started_epoch(timeline) - a.skew_ms / 1000.0 - a.capture_start
    duration = _duration(a.ffprobe, a.raw)
    end_at = None if a.window_gone is None or a.capture_start is None else a.window_gone - a.capture_start
    p = plan(timeline, cards, a.offset, duration, a.threshold, a.keep, a.lead, a.hold, a.card_s, end_at)
    p["raw_duration"] = duration
    if a.plan_out:
        write_plan(p, a.plan_out)
    log(f"cut-idle: offset {p['offset']:.3f} s, {len(p['cuts'])} cuts, {len(p['cards'])} cards, "
        f"{p['out_duration']:.1f} s from {duration:.1f} s")
    if a.plan_only:
        return 0

    with tempfile.TemporaryDirectory() as work:
        names = []
        for i, c in enumerate(p["cards"]):
            for kind in ("title", "subtitle"):
                name = f"card{i}-{kind}.txt"
                pathlib.Path(work, name).write_text(c[kind], encoding="utf-8")
                names.append(name)
        pathlib.Path(work, "vf.txt").write_text(build_filter(p, a.fps, a.font, names), encoding="utf-8")
        cmd = [a.ffmpeg, "-hide_banner", "-loglevel", "warning", "-y", "-i", os.path.abspath(a.raw),
               "-/vf", "vf.txt", "-an", "-c:v", "libx264", "-preset", "medium", "-crf", "23",
               "-pix_fmt", "yuv420p", "-movflags", "+faststart", os.path.abspath(a.out)]
        subprocess.run(cmd, check=True, cwd=work)
    log(f"cut-idle: wrote {a.out}")
    return 0


if __name__ == "__main__":
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8", errors="replace")
    sys.exit(main())
