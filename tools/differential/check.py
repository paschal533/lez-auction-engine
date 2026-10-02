import json
import sys
from collections import Counter

from model import RAY, bid, mul_div_ceil, mul_div_floor, rai_discount

KINDS = ("md", "sch", "bid", "bid_lot", "bid_ff")
BID_KINDS = ("bid", "bid_lot", "bid_ff")


def s(v):
    return None if v is None else int(v)


def check_one(v, price):
    k = v["k"]
    if k == "md":
        x, y, d = int(v["x"]), int(v["y"]), int(v["d"])
        return s(v["f"]) == mul_div_floor(x, y, d) and s(v["c"]) == mul_div_ceil(x, y, d)
    if k == "sch":
        return int(v["d"]) == rai_discount(v["t"])
    if k in BID_KINDS:
        out, charged = bid(int(v["coin"]), v["t"], price, int(v["lot"]), int(v["remaining"]))
        return (int(v["out"]), int(v["charged"])) == (out, charged)
    raise ValueError(f"unknown vector kind {k!r}")


def main(path):
    with open(path) as fh:
        vec = json.load(fh)
    price = RAY * 752 // 1_000_000
    counts = Counter(v["k"] for v in vec)
    bad = 0
    for v in vec:
        if not check_one(v, price):
            bad += 1
            print("MISMATCH", v)
    empty = [k for k in KINDS if counts[k] == 0]
    per_kind = ", ".join(f"{k}={counts[k]}" for k in KINDS)
    print(f"{len(vec)} vectors ({per_kind}), {bad} mismatches")
    if empty:
        print("EMPTY KINDS:", ", ".join(empty))
    sys.exit(1 if bad or empty else 0)


if __name__ == "__main__":
    main(sys.argv[1])
