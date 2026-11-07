#!/usr/bin/env python3
"""Generate docs/architecture/refusal-codes.md: every EngineError refusal and the test that proves it
(Plan 8 Task 4).

usage: gen-refusal-codes.py [--root DIR] [--check]
  (no flag)  write docs/architecture/refusal-codes.md under DIR (default: the current directory)
  --check    exit 1 if the doc is stale, a variant lacks a MEANINGS line, a variant has no proving test
             and is not in KNOWN_GAPS, or a KNOWN_GAPS entry now has a test (exit 2 on bad usage)

Source of truth: programs/auction_engine/core/src/error.rs (`EngineError` + `code()` arms); the 0.4
engine shares the enum through crates/auction_engine_params. A test proves a variant when a #[test] fn
(attributes included, so #[should_panic(expected = ..)] counts) names `EngineError::Variant`, the wire
form `[60NN]` / `Program error NN`, or the bare quoted name ("Variant", "...: Variant") that the
integration tests assert on. Only programs/, v04/ and crates/ are scanned; target dirs are skipped.
"""
import importlib.util
import os
import pathlib
import re
import sys

ERROR_RS_PATH = "programs/auction_engine/core/src/error.rs"
DOC_PATH = "docs/architecture/refusal-codes.md"
SCAN_DIRS = ("programs", "v04", "crates")
SKIP_DIRS = {"target", "node_modules", ".git"}
WIRE_BASE = 6000
MAX_TESTS, MAX_PATHS = 4, 3

_spec = importlib.util.spec_from_file_location(
    "count_tests", pathlib.Path(__file__).resolve().parent / "count-tests.py")
_ct = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_ct)
mask_rust = _ct.mask_rust

# Tests that only check .code() / From mappings / message format, so they never prove a refusal.
NOT_PROOF = {"error_codes_are_stable", "fail_text_matches_v024_format"}

# One short line per variant, written from the raising code.
MEANINGS = {
    'NotAdmin': 'The signer is not the configured admin.',
    'ZeroId': 'A required account or token id is all zeros.',
    'ZeroRefPrice': 'The reference (redemption) price is zero.',
    'ThresholdBelowDebtLot': 'The debt-auction threshold is below the debt lot size.',
    'InvalidConfig': 'The parameter set fails validation (ranges, ordering, caps or schedule).',
    'TypeCapAboveGlobal': 'A per-collateral cap exceeds the global cap.',
    'BpsOutOfRange': 'A basis-point field is above 10,000.',
    'ScheduleEncoding': 'The discount-schedule words are malformed or the wrong length.',
    'TtlOutOfRange': 'A time-to-live or maximum-age field is outside its allowed range.',
    'ChipAbovePenalty': 'The keeper chip is larger than the liquidation penalty.',
    'HostMismatch': 'The host protocol-parameters account is not the one the engine is bound to.',
    'WrongAccount': 'A supplied account has the wrong id, owner, contents or signer flag (the name of the slot follows in the message).',
    'OracleMismatch': "The supplied oracle account is not the one the host's health gate uses.",
    'WrongClock': 'The clock account is not the system clock or does not decode.',
    'ProtocolTokenLocked': 'The protocol token, the reference oracle and the system parameter set cannot change while a debt or surplus auction is live.',
    'RefPriceStepTooLarge': 'A reference-price update moves further than the allowed step.',
    'Paused': 'The engine is paused by the admin.',
    'PriceFeedStale': 'The price feed is older than its maximum age, which pauses the engine.',
    'PriceFeedLowConfidence': "The price feed's confidence is below the minimum, which pauses the engine.",
    'PriceFeedOutOfRange': 'The price feed reports zero or an out-of-range price, which pauses the engine.',
    'NonceMismatch': 'The auction nonce is not the next expected one.',
    'ZeroDebt': 'The position has no debt to liquidate.',
    'VaultBelowCollateral': 'The position vault holds less than the collateral the position books.',
    'CapacityExhausted': 'The global or per-type liquidation capacity has no room for this lot.',
    'NotAuctioning': 'The auction is not in its bidding phase (not yet seized, or already settled).',
    'ZeroFill': 'The bid fills nothing.',
    'Slippage': "The fill is worse than the bidder's stated minimum.",
    'BelowMinBid': 'The bid is below the minimum bid size.',
    'DustRemainder': 'The bid would leave a remainder below the dust threshold.',
    'NotOwner': 'The caller does not own the position or claim.',
    'NothingToClaim': 'There is no leftover collateral to claim.',
    'ChainEndAccountsMissing': 'The final bid of an auction omits the accounting account it must update.',
    'InsufficientBalance': "The bidder's or taker's holding cannot cover the payment.",
    'RecipientMustSign': 'A recipient holding that does not exist yet must sign the transaction.',
    'WrongDefinition': 'A token holding is not the expected fungible token definition or is not owned by the token program.',
    'VaultBelowBook': 'The auction vault holds less collateral than the fill or claim needs.',
    'TokenNotConfigured': 'The protocol token needed for a system auction is not configured.',
    'UnbackedDebtPresent': 'A surplus auction cannot start while unbacked debt remains.',
    'BufferBelowTarget': 'The surplus buffer is below the target needed to start a surplus auction.',
    'DelayNotElapsed': 'The required delay has not elapsed.',
    'InsufficientBadDebt': 'Unbacked debt net of the buffer is smaller than the debt-auction lot.',
    'MintCapConfig': 'The protocol-token mint cap or reference price cannot mint a useful amount.',
    'MintAuthorityMissing': 'The engine does not hold mint authority over the protocol token.',
    'ProtocolTokenMismatch': 'The protocol-token account is not a fungible definition of the configured token program.',
    'DebtAuctionAlreadyLive': 'A debt auction is already live.',
    'SurplusAuctionAlreadyLive': 'A surplus auction is already live.',
    'NotLive': 'The system auction is not live.',
    'RecoveryInProgress': 'A live debt auction (recovery) blocks this action.',
    'Math': 'Arithmetic overflow, underflow or division by zero.',
}

# Variants with no proving test yet: variant -> reason and the plan that closes it.
KNOWN_GAPS = {
    "VaultBelowCollateral": "Core refusal tested in auction_core (crates/auction_core/src/liquidation.rs:164-167); "
                            "no engine-level test drives it to wire code 6032; engine-level test planned in Tier 2 (M1).",
}

CODE_ARM = re.compile(r"Self::(\w+)(?:\([^)]*\))?\s*=>\s*(\d+)\s*,")
TEST_ATTR = re.compile(r"#\[(?:[A-Za-z_]\w*::)*test(?:\s*\([^\]]*\))?\]")
FN_NAME = re.compile(r"\bfn\s+(\w+)")
CFG_TEST_MOD = re.compile(r"#\[cfg\(test\)\]\s*(?:pub\s+)?mod\s+\w+\s*\{")


def parse_codes(text):
    """[(Variant, code)] from the `code()` match arms, in source order; rejects duplicate codes."""
    start = text.find("fn code(")
    arms = [(m.group(1), int(m.group(2))) for m in CODE_ARM.finditer(text, max(start, 0))]
    seen = {}
    for name, code in arms:
        if code in seen:
            raise ValueError(f"code {code} used by both {seen[code]} and {name}")
        seen[code] = name
    return arms


def match_brace(masked, open_idx):
    """Index just past the `}` closing the `{` at open_idx (masked text has no braces in strings)."""
    depth = 0
    for i in range(open_idx, len(masked)):
        if masked[i] == "{":
            depth += 1
        elif masked[i] == "}":
            depth -= 1
            if depth == 0:
                return i + 1
    return len(masked)


def test_fns(raw):
    """[(name, raw text from the attribute to the end of the body)] for every real #[test] fn."""
    masked = mask_rust(raw)
    out = []
    for attr in TEST_ATTR.finditer(masked):
        m = FN_NAME.search(masked, attr.end())
        if not m:
            continue
        brace = masked.find("{", m.end())
        if brace < 0:
            continue
        out.append((m.group(1), raw[attr.start():match_brace(masked, brace)]))
    return out


def strip_test_mods(raw):
    """Raw text with every `#[cfg(test)] mod x { .. }` removed, so inline unit tests are not 'raised in'."""
    masked = mask_rust(raw)
    out, pos = [], 0
    for m in CFG_TEST_MOD.finditer(masked):
        if m.start() < pos:
            continue
        end = match_brace(masked, m.end() - 1)
        out.append(raw[pos:m.start()])
        pos = end
    out.append(raw[pos:])
    return "".join(out)


def is_test_path(rel):
    parts = rel.split("/")
    return "tests" in parts[:-1] or parts[-1].endswith("_tests.rs") or parts[-1] == "tests.rs"


def rust_files(root):
    for top in SCAN_DIRS:
        for dirpath, dirnames, files in os.walk(os.path.join(root, top)):
            dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
            for f in sorted(files):
                if f.endswith(".rs"):
                    full = os.path.join(dirpath, f)
                    yield os.path.relpath(full, root).replace(os.sep, "/"), full


def read(path):
    with open(path, encoding="utf-8-sig", errors="replace") as f:
        return f.read()


def build_index(root):
    """(non_test_sources {rel: masked text}, tests [(rel, name, text)]) over programs/, v04/, crates/."""
    sources, tests = {}, []
    for rel, full in rust_files(root):
        raw = read(full)
        tests.extend((rel, n, body) for n, body in test_fns(raw))
        if rel != ERROR_RS_PATH and not is_test_path(rel):
            sources[rel] = mask_rust(strip_test_mods(raw))
    return sources, tests


def proof_pattern(variant, code):
    alts = [rf"EngineError::{variant}\b", rf'"(?:[^"\n]*:\s*)?{variant}(?:\(|")']
    if code is not None:
        alts += [rf"\[{WIRE_BASE + code}\]", rf"Program error {code}\b"]
    return re.compile("|".join(alts))


def engine_label(rel):
    """Which engine a test path belongs to: (sdk), (0.4), (v0.2.4) or (core)."""
    if rel.startswith("v04/sdk/") or rel.startswith(("crates/auction_sdk/", "crates/auction_ffi/")):
        return "sdk"
    if rel.startswith("v04/"):
        return "0.4"
    if rel.startswith("programs/"):
        return "v0.2.4"
    return "core"


def find_uses(root, variant, code=None, index=None, error_text=""):
    """{"raised_in": [paths], "tests": [fn names that prove it], "labelled": ["(engine) name"]}.

    code (the engine code) enables wire-form matching. NOT_PROOF fns are skipped; (sdk) tests predict
    a refusal rather than prove the program raises it, so they never count as proof."""
    sources, tests = index or build_index(root)
    raise_re = re.compile(rf"EngineError::{variant}\b")
    pat = proof_pattern(variant, code)
    seen, names, labelled = set(), [], []
    for rel, name, body in tests:
        label = engine_label(rel)
        if name in NOT_PROOF or label == "sdk" or not pat.search(body) or (label, name) in seen:
            continue
        seen.add((label, name))
        labelled.append(f"({label}) {name}")
        if name not in names:
            names.append(name)
    raised = {r for r, t in sources.items() if raise_re.search(t)}
    core_names = from_arms(error_text).get(variant, [])
    if core_names:
        core_re = re.compile(r"\b(?:" + "|".join(re.escape(n) for n in core_names) + r")\b")
        raised |= {r for r, t in sources.items() if core_re.search(t)}
        if variant == "Math":
            raised.add(ERROR_RS_PATH)
    return {"raised_in": sorted(raised), "tests": names, "labelled": labelled}


def capped(items, limit):
    shown = ", ".join(f"`{i}`" for i in items[:limit])
    return shown + (f" and {len(items) - limit} more" if len(items) > limit else "")


FROM_ARM = re.compile(r"((?:\w+::\w+\s*\|\s*)*\w+::\w+)\s*=>\s*Self::(\w+)")


def from_arms(error_text):
    """{EngineVariant: ["CoreError::Name", ...]} from the `impl From<..> for EngineError` match arms
    (`A::X | A::Y => Self::V`), so renamed mappings (PauseReason::Stale -> PriceFeedStale) are followed."""
    start = error_text.find("impl From<")
    out = {}
    if start < 0:
        return out
    for m in FROM_ARM.finditer(error_text, start):
        found = out.setdefault(m.group(2), [])
        found.extend(n for n in (x.strip() for x in m.group(1).split("|")) if n not in found)
    return out


def render(root):
    """(doc text, problems list) for the tree at root."""
    error_text = read(os.path.join(root, ERROR_RS_PATH))
    codes = parse_codes(error_text)
    index = build_index(root)
    rows, problems, gaps = [], [], []
    for name, code in codes:
        uses = find_uses(root, name, code, index, error_text)
        raised = capped(uses["raised_in"], MAX_PATHS) if uses["raised_in"] else "-"
        proved = capped(uses["labelled"], MAX_TESTS) if uses["labelled"] else "**no test**"
        if not uses["raised_in"] and from_arms(error_text).get(name):
            problems.append(f"{name}: mapped by a From impl but no raise site found")
        if name not in MEANINGS:
            problems.append(f"{name}: no MEANINGS line")
        if not uses["tests"]:
            if name in KNOWN_GAPS:
                gaps.append((name, KNOWN_GAPS[name]))
            else:
                problems.append(f"{name}: no proving test and not in KNOWN_GAPS")
        elif name in KNOWN_GAPS:
            problems.append(f"{name}: has a test but is still in KNOWN_GAPS")
        rows.append(f"| {WIRE_BASE + code} | `{name}` | {code} | {raised} | {proved} | {MEANINGS.get(name, '')} |")
    lines = [
        "# Refusal codes", "",
        "<!-- generated by scripts/gen-refusal-codes.py; do not edit by hand -->", "",
        f"Every `EngineError` the auction engine can refuse with. The wire code is `{WIRE_BASE} + engine code`; "
        "a failed transaction reads `Program error [6001]: Program error 1: NotAdmin`. The v0.2.4 programs "
        "and the 0.4 engine share this enum (through `crates/auction_engine_params`), so one catalogue "
        "covers both. `Proved by` lists the tests whose body names the variant or its wire form.", "",
        "| Wire code | Variant | Engine code | Raised in | Proved by | Meaning |",
        "|---:|---|---:|---|---|---|", *rows, "", "## Gaps", ""]
    if gaps:
        lines += ["Variants with no proving test yet:", ""] + [f"- `{n}`: {why}" for n, why in gaps]
    else:
        lines.append("None: every variant above has at least one proving test.")
    return "\n".join(lines) + "\n", problems


def main(argv):
    args, root, check = argv[1:], ".", False
    try:
        while args:
            a = args.pop(0)
            if a == "--root":
                root = args.pop(0)
            elif a == "--check":
                check = True
            else:
                raise ValueError(a)
    except (IndexError, ValueError):
        print(__doc__, file=sys.stderr)
        return 2
    try:
        doc, problems = render(root)
    except (OSError, ValueError) as e:
        print(f"gen-refusal-codes: {e}", file=sys.stderr)
        return 2
    path = os.path.join(root, DOC_PATH)
    if not check:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(doc)
        return 0
    try:
        current = read(path)
    except OSError as e:
        print(f"gen-refusal-codes: cannot read {path}: {e}", file=sys.stderr)
        return 1
    if current != doc:
        problems.append(f"{DOC_PATH} is stale; run scripts/gen-refusal-codes.py")
    for p in problems:
        print(f"gen-refusal-codes: {p}", file=sys.stderr)
    if not problems:
        print("gen-refusal-codes: the catalogue is current")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
