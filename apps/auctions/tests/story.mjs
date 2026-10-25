#!/usr/bin/env node
// The whole demo story as UI actions in Basecamp (Plan 4 Task 16): every step of scripts/lib/demo-scenario.sh
// after the setup phases, in its order (R39), driven through the QML inspector with the matching account picked
// in the header. After the steps that move the books it reads the figures from the screen (objectName) and
// checks them against ADR-051's expected values, cross-checked with auction-cli on the same state.
// Run it with scripts/basecamp/run-app.sh --setup --test <this file> (fresh localnet, positions P0–P2 open).
//
// Environment (run-app.sh sets the first five):
//   LOGOS_QT_MCP          logos-qt-mcp package root (test-framework/framework.mjs)
//   AUCTION_CLI           auction-cli binary
//   AUCTIONS_DEPLOYMENT   the run's deployment file; its directory holds run.env (P0, P2) from the setup
//   AUCTIONS_STATE_DIR, LEE_WALLET_HOME_DIR   as the app
//   STORY_PACE_MS         optional pause after every step, for a recording (default 0)
//   STORY_TIMELINE        optional: where story-timeline.json goes (default next to the deployment)
//   STORY_SHOTS_DIR       optional: screenshots at key steps (default <deployment dir>/story-shots)
//   STORY_FROM            optional: start at this step id (a resumed run on the same localnet)
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

import { FULL_STORY } from "./lib/story-steps.mjs";

const APP_READY_MS = 120000;
const FIND_MS = 20000;
const POLL_MS = 500;
const DEFAULT_WAIT_MS = 10000;
// A figure read after a transaction: the screen re-reads its overview when the lane reaches done.
const FIGURE_MS = 30000;
// ADR-051 (scripts/lib/demo-scenario.sh): the expected values of the demo on a fresh localnet.
const EXPECT = {
  bufferAfterF1F3: 30000n,
  bufferAfterSurplusTake: 29500n,
  unbackedLow: 42154n,
  unbackedHigh: 63200n,
  settledByBuffer: 29500n,
  debtLot: 5000n,
  freezeThreshold: 5000n,
  // The taker's protocol balance before the debt take (10,000 less the surplus burn); after it must be above.
  tokensMintedAbove: 9800n,
};
// Notices whose message explains a step that did not happen (shown in the failure report).
const NOTICES = ["errorNotice", "accountsNotice", "systemActionNotice", "systemNotice", "devActionNotice",
  "devNotice", "devPriceQuoteNotice", "liquidateNotice", "riskNotice", "bidNotice", "bidPausedNotice", "liveNotice",
  "claimNotice", "settledNotice", "settleQuoteNotice", "takeDebtQuoteNotice", "takeSurplusQuoteNotice", "nodeNotice"];
// Lanes and the texts that say why one stopped.
const LANE_PREFIXES = { txLane: "", riskTxLane: "risk", settledTxLane: "settled", systemTxLane: "system", devTxLane: "dev" };
// Screenshots after these steps (the story's key moments).
const SHOTS = new Set(["shell-header-shows-keeper", "liquidate-included", "bid-final-settles", "claim-claimed",
  "system-take-surplus-included", "dev-publish-price-quote", "dev-publish-price-included", "bid-shortfall-included",
  "system-take-debt-quote", "dev-disable-token-dialog", "story-frozen"]);

function need(name) {
  const value = process.env[name];
  if (!value) throw new Error(`set ${name} (scripts/basecamp/run-app.sh does)`);
  return value;
}

// ---- The story is data; refuse one that cannot be timed or resumed by id. ----
const duplicates = FULL_STORY.map((s) => s.id).filter((id, i, all) => all.indexOf(id) !== i);
if (duplicates.length > 0) {
  console.error(`story: duplicate step ids: ${[...new Set(duplicates)].join(", ")}`);
  process.exit(1);
}

const DEP_DIR = dirname(need("AUCTIONS_DEPLOYMENT"));
const PACE_MS = Number(process.env.STORY_PACE_MS || 0);
const TIMELINE = process.env.STORY_TIMELINE || join(DEP_DIR, "story-timeline.json");
const SHOTS_DIR = process.env.STORY_SHOTS_DIR || join(DEP_DIR, "story-shots");

const { test, run } = await import(`${need("LOGOS_QT_MCP")}/test-framework/framework.mjs`);

// ---- auction-cli, the reference for every figure ----

function cli(...args) {
  return execFileSync(need("AUCTION_CLI"), [
    "--deployment", need("AUCTIONS_DEPLOYMENT"),
    "--state-dir", need("AUCTIONS_STATE_DIR"),
    "--wallet-home", need("LEE_WALLET_HOME_DIR"),
    ...args,
  ], { encoding: "utf-8", stdio: ["ignore", "pipe", "pipe"] });
}

/** The last field of the first `key  value` line (scripts/lib/localnet.sh `field`). */
function field(out, key) {
  const line = out.split("\n").find((l) => l.startsWith(key));
  if (!line) throw new Error(`auction-cli printed no "${key}" line:\n${out}`);
  return line.trim().split(/\s+/).pop();
}

let bufferId = "";
const cliRef = {
  balance: (account) => BigInt(field(cli("dev", "balance", "--account", account), "balance")),
  buffer() {
    if (!bufferId) bufferId = field(cli("admin", "addresses", "--admin", "engine-admin", "--salt", "1"), "buffer");
    return this.balance(bufferId);
  },
  unbacked() {
    const out = cli("config");
    const m = /^accounting\s+unbacked debt (\d+),/m.exec(out);
    if (!m) throw new Error(`auction-cli config printed no unbacked debt:\n${out}`);
    return BigInt(m[1]);
  },
  frozen: () => field(cli("host", "status"), "frozen"),
  // Live auction ids (list-auctions; reads the same scan cursor as the app).
  liveAuctions: () => cli("list-auctions").split("\n").filter((l) => /\s{2}live\s{2}/.test(l)).map((l) => l.split(/\s+/)[0]),
};

// ---- Run-time values: positions from the setup's run.env, auctions captured after each liquidation ----

const captured = {};
function readRunEnv() {
  const env = readFileSync(join(DEP_DIR, "run.env"), "utf-8");
  for (const [key, name] of [["P0", "position:p0"], ["P2", "position:p2"]]) {
    const values = env.split("\n").filter((l) => l.startsWith(`${key}=`)).map((l) => l.slice(key.length + 1).trim());
    if (values.length === 0) throw new Error(`run.env has no ${key} (run with --setup)`);
    captured[name] = values[values.length - 1];
  }
}

function fill(text) {
  return String(text).replace(/\{\{([a-z]+:[a-z0-9]+)\}\}/g, (_, key) => {
    if (!(key in captured)) throw new Error(`no value yet for {{${key}}}`);
    return captured[key];
  });
}

// ---- Inspector helpers ----

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function matches(app, objectName) {
  const res = await app.findByProperty("objectName", objectName);
  if (res.error || !res.matches) return [];
  return res.matches;
}

async function props(app, id) {
  const res = await app.getProperties(id);
  if (res.error) throw new Error(`getProperties: ${res.error}`);
  return res.properties;
}

function propValue(properties, name) {
  const found = properties.find((p) => p.name === name);
  return found ? found.value : undefined;
}

/** The first item named `objectName`; with `visibleOnly`, the first one that is visible. */
async function item(app, objectName, { visibleOnly = false, timeoutMs = FIND_MS } = {}) {
  const start = Date.now();
  let seen = 0;
  while (Date.now() - start < timeoutMs) {
    const found = await matches(app, objectName);
    seen = found.length;
    for (const m of found) {
      if (!visibleOnly) return m.id;
      try {
        if (propValue(await props(app, m.id), "visible") === true) return m.id;
      } catch { /* destroyed since the search: look again */ }
    }
    await sleep(POLL_MS);
  }
  throw new Error(seen === 0 ? `no item named ${objectName}` : `${objectName} is not visible`);
}

async function read(app, objectName, name) {
  const value = propValue(await props(app, await item(app, objectName)), name);
  if (value === undefined) throw new Error(`${objectName} has no property ${name}`);
  return value;
}

/** Polls `objectName.name` until `ok(value)`; returns the last value either way. */
async function poll(app, objectName, name, ok, timeoutMs) {
  const start = Date.now();
  let value;
  for (;;) {
    try { value = await read(app, objectName, name); } catch { value = undefined; }
    if (value !== undefined && ok(value)) return { ok: true, value };
    if (Date.now() - start >= timeoutMs) return { ok: false, value };
    await sleep(POLL_MS);
  }
}

async function shot(app, id) {
  try {
    const res = await app.screenshot();
    if (res.error || !res.image) throw new Error(res.error ?? "no image");
    mkdirSync(SHOTS_DIR, { recursive: true });
    writeFileSync(join(SHOTS_DIR, `${id}.png`), Buffer.from(res.image, "base64"));
  } catch (err) {
    console.log(`    (screenshot ${id} failed: ${err.message})`);
  }
}

/** What the screen says about a failure: lane texts and every shown notice. */
async function diagnose(app) {
  const lines = [];
  for (const name of NOTICES) {
    try {
      const p = await props(app, await item(app, name, { timeoutMs: 1 }));
      if (propValue(p, "shown") === true && propValue(p, "message")) lines.push(`${name}: ${propValue(p, "message")}`);
    } catch { /* not on screen */ }
  }
  for (const [lane, prefix] of Object.entries(LANE_PREFIXES)) {
    try {
      const state = await read(app, lane, "state");
      if (state && state !== "") {
        const scoped = (b) => (prefix ? prefix + b.charAt(0).toUpperCase() + b.slice(1) : b);
        const title = await read(app, scoped("txLaneTitle"), "text").catch(() => "");
        const body = await read(app, scoped("txLaneBody"), "text").catch(() => "");
        lines.push(`${lane}: ${state} · ${title} · ${body}`);
      }
    } catch { /* no lane */ }
  }
  return lines.length ? `\n      ${lines.join("\n      ")}` : "";
}

// ---- Steps ----

// A Repeater rebuilds its delegates when its model is replaced (a list row after a fresh overview), so an id found a
// moment ago can be gone: find it again and retry.
const STALE_TRIES = 5;

async function click(app, objectName) {
  for (let attempt = 1; ; attempt++) {
    const id = await item(app, objectName, { visibleOnly: true });
    let res;
    try {
      const enabled = propValue(await props(app, id), "enabled");
      if (enabled === false) throw new Error(`${objectName} is disabled`);
      res = await app.inspector.send("click", { objectId: id });
    } catch (err) {
      if (attempt < STALE_TRIES && /not found/i.test(err.message)) { await sleep(POLL_MS); continue; }
      throw err;
    }
    if (!res.error) return;
    if (attempt < STALE_TRIES && /not found/i.test(res.error)) { await sleep(POLL_MS); continue; }
    throw new Error(`click(${objectName}): ${res.error}`);
  }
}

async function setText(app, objectName, text) {
  for (let attempt = 1; ; attempt++) {
    const id = await item(app, objectName, { visibleOnly: true });
    const res = await app.inspector.send("setProperty", { objectId: id, property: "text", value: text });
    if (!res.error && res.value === text) return;
    if (attempt < STALE_TRIES && /not found/i.test(String(res.error))) { await sleep(POLL_MS); continue; }
    throw new Error(`could not set ${objectName}.text: ${res.error ?? res.value}`);
  }
}

async function waitFor(app, step, timing) {
  const property = step.property || "text";
  const want = String(step.value);
  const timeoutMs = step.timeoutMs || DEFAULT_WAIT_MS;
  const isLane = step.target in LANE_PREFIXES;
  const start = Date.now();
  let last;
  while (Date.now() - start < timeoutMs) {
    try { last = await read(app, fill(step.target), property); } catch { last = undefined; }
    if (last !== undefined && String(last) === want) {
      if (isLane) timing.lane_wait_ms = Date.now() - start;
      return;
    }
    // A lane that dropped will not reach done; say why now instead of at the timeout.
    if (isLane && want === "done" && last === "dropped") throw new Error(`${step.target} dropped`);
    await sleep(POLL_MS);
  }
  throw new Error(`${step.target}.${property} is ${JSON.stringify(last)}, expected ${want} within ${timeoutMs / 1000} s`);
}

async function runStep(app, step) {
  const target = step.target ? fill(step.target) : "";
  const timing = {};
  switch (step.action) {
    case "click": await click(app, target); break;
    case "setText": await setText(app, target, fill(step.value)); break;
    case "waitFor": await waitFor(app, step, timing); break;
    default: throw new Error(`unknown action ${step.action}`);
  }
  return timing;
}

// ---- ADR-051 checks, read from the screen and cross-checked with auction-cli ----

const results = [];
const memo = {};

function record(name, uiValue, cliValue, expected, ok) {
  results.push({ check: name, ui: String(uiValue), cli: String(cliValue), expected, ok });
  console.log(`    check ${name}: ui ${uiValue}, cli ${cliValue}, expected ${expected}: ${ok ? "ok" : "FAILED"}`);
  if (!ok) throw new Error(`check ${name} failed: ui ${uiValue}, cli ${cliValue}, expected ${expected}`);
}

const digits = (v) => BigInt(String(v).replace(/,/g, ""));

/** A System figure (a Fact's raw `value`) once the screen shows `expected`, or its last value. */
async function systemFigure(app, objectName, ok) {
  await click(app, "tabSystem");
  const res = await poll(app, objectName, "value", (v) => /^\d+$/.test(String(v)) && ok(BigInt(v)), FIGURE_MS);
  return res.value === undefined ? undefined : BigInt(res.value);
}

const CHECKS = {
  // The auction id each liquidation opened: the live auction auction-cli lists that was not live before.
  "liquidate-rescan-done": async () => {
    const live = cliRef.liveAuctions();
    if (live.length !== 1) throw new Error(`expected one live auction after P0's liquidation, auction-cli lists ${live.length}`);
    captured["auction:p0"] = live[0];
    console.log(`    P0's auction: ${live[0]}`);
  },
  "liquidate-2-rescan-done": async () => {
    const live = cliRef.liveAuctions().filter((id) => id !== captured["auction:p0"]);
    if (live.length !== 1) throw new Error(`expected one new live auction after P2's liquidation, auction-cli lists ${live.length}`);
    captured["auction:p2"] = live[0];
    console.log(`    P2's auction: ${live[0]}`);
  },
  "claim-claimed": async (app) => {
    const ui = await systemFigure(app, "surplusBuffer", (v) => v === EXPECT.bufferAfterF1F3);
    const ref = cliRef.buffer();
    record("buffer holds the 10 % penalty (F1–F3)", ui, ref, "30000", ui === EXPECT.bufferAfterF1F3 && ref === ui);
  },
  "system-take-surplus-included": async (app) => {
    const ui = await systemFigure(app, "surplusBuffer", (v) => v === EXPECT.bufferAfterSurplusTake);
    const ref = cliRef.buffer();
    record("buffer after the surplus lot", ui, ref, "29500", ui === EXPECT.bufferAfterSurplusTake && ref === ui);
  },
  "bid-shortfall-included": async (app) => {
    const inRange = (v) => v >= EXPECT.unbackedLow && v <= EXPECT.unbackedHigh;
    const ui = await systemFigure(app, "debtUnbacked", inRange);
    const ref = cliRef.unbacked();
    memo.u1 = ui;
    record("unbacked debt after the shortfall", ui, ref, "[42154, 63200]", ui !== undefined && inRange(ui) && ref === ui);
  },
  "system-settle-debt-2-included": async (app) => {
    const want = memo.u1 - EXPECT.settledByBuffer;
    const ui = await systemFigure(app, "debtUnbacked", (v) => v === want);
    const ref = cliRef.unbacked();
    memo.u2 = ui;
    record("the buffer settled 29,500 of the bad debt", ui, ref, String(want), ui === want && ref === ui && ui >= EXPECT.debtLot);
  },
  "system-take-debt-quote": async (app) => {
    const res = await poll(app, "takeDebtGet", "text", (v) => /^[\d,]+$/.test(String(v)), DEFAULT_WAIT_MS);
    memo.quotedTokens = res.ok ? digits(res.value) : undefined;
    memo.tokensBefore = cliRef.balance("taker-protocol");
  },
  // ADR-051 / demo-scenario.sh: the taker's protocol balance after the take is above its balance before (9,800
  // after the surplus burn; the legacy-testnet run recorded "13050 gt 9800"). The screen's quote names the tokens
  // minted; the balance rises by exactly that.
  "system-take-debt-included": async (app) => {
    const after = cliRef.balance("taker-protocol");
    const uiAfter = memo.quotedTokens === undefined ? undefined : memo.tokensBefore + memo.quotedTokens;
    record("protocol tokens minted to the taker (balance before -> after)", uiAfter, after,
      `> ${memo.tokensBefore} (${EXPECT.tokensMintedAbove})`,
      uiAfter === after && memo.tokensBefore === EXPECT.tokensMintedAbove && after > EXPECT.tokensMintedAbove);
    const want = memo.u2 - EXPECT.debtLot;
    const ui = await systemFigure(app, "debtUnbacked", (v) => v === want);
    const ref = cliRef.unbacked();
    record("one lot of bad debt burned", ui, ref, String(want),
      ui === want && ref === ui && ui > EXPECT.freezeThreshold);
  },
  "story-frozen": async (app) => {
    const ui = await read(app, "headerFrozen", "visible");
    const ref = cliRef.frozen();
    record("host frozen", ui ? "yes" : "no", ref, "yes", ui === true && ref === "yes");
  },
};

// ---- The run ----

test("story: the whole demo runs in Basecamp, figures match ADR-051 and auction-cli", async (app) => {
  readRunEnv();
  const timeline = { started: new Date().toISOString(), pace_ms: PACE_MS, steps: [], checks: results };
  const save = () => writeFileSync(TIMELINE, JSON.stringify(timeline, null, 2) + "\n");
  const t0 = Date.now();

  // The app is ready once configure and openWallet succeeded on a verified dev network.
  await app.waitFor(async () => {
    if ((await read(app, "tabDev", "visible")) !== true) throw new Error("Dev tools hidden");
  }, { timeout: APP_READY_MS, interval: POLL_MS * 2, description: "Dev tools tab visible" });

  const from = process.env.STORY_FROM ? FULL_STORY.findIndex((s) => s.id === process.env.STORY_FROM) : 0;
  if (from < 0) throw new Error(`STORY_FROM: no step ${process.env.STORY_FROM}`);
  try {
    for (const step of FULL_STORY.slice(from)) {
      const entry = { id: step.id, segment: step.segment, caption: step.caption, action: step.action,
        target: step.target, start_ms: Date.now() - t0 };
      timeline.steps.push(entry);
      try {
        Object.assign(entry, await runStep(app, step));
        entry.end_ms = Date.now() - t0;
        console.log(`  · ${step.id} (${((entry.end_ms - entry.start_ms) / 1000).toFixed(1)} s)`);
        if (SHOTS.has(step.id)) await shot(app, step.id);
        if (CHECKS[step.id]) await CHECKS[step.id](app);
      } catch (err) {
        entry.end_ms = Date.now() - t0;
        entry.error = err.message;
        await shot(app, `FAILED-${step.id}`);
        throw new Error(`step ${step.id}: ${err.message}${await diagnose(app)}`);
      }
      if (PACE_MS > 0) await sleep(PACE_MS);
    }
  } finally {
    timeline.total_ms = Date.now() - t0;
    save();
    console.log(`    timeline: ${TIMELINE} (${(timeline.total_ms / 1000).toFixed(0)} s)`);
  }
});

run();
