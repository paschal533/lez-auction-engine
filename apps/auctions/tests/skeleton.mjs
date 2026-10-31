#!/usr/bin/env node
// Skeleton end-to-end (Plan 4 Task 6, ADR-018): the Auctions app in Basecamp sends a real token transfer
// through lez_core on a localnet. Run it with scripts/basecamp/run-app.sh --setup --test <this file>,
// which starts the localnet, launches Basecamp and sets the environment below.
//
// Environment:
//   LOGOS_QT_MCP          logos-qt-mcp package root (test-framework/framework.mjs)
//   AUCTION_CLI           auction-cli binary
//   AUCTIONS_DEPLOYMENT   the run's deployment file; AUCTIONS_STATE_DIR, LEE_WALLET_HOME_DIR as the app
//   SKELETON_SCREENSHOT   optional: PNG path for a screenshot of the header and the five tabs
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";

const FROM = "owner-stable";
const TO = "taker-stable";
const AMOUNT = 1n;
const APP_READY_MS = 90000;
const DONE_MS = 120000;
const POLL_MS = 1000;
// Status texts of a transfer still on its way (TransferDetail.qml); any other text but "Done" is a failure.
const IN_PROGRESS = [/^Sending/, /^Sent\./, /^Step \d+ of \d+ sent\./];

function need(name) {
  const value = process.env[name];
  if (!value) throw new Error(`set ${name} (scripts/basecamp/run-app.sh does)`);
  return value;
}

const { test, run } = await import(`${need("LOGOS_QT_MCP")}/test-framework/framework.mjs`);

/** The taker's balance as auction-cli prints it (`balance  <n>`), the CLI being the reference. */
function cliBalance(label) {
  const out = execFileSync(need("AUCTION_CLI"), [
    "--deployment", need("AUCTIONS_DEPLOYMENT"),
    "--state-dir", need("AUCTIONS_STATE_DIR"),
    "--wallet-home", need("LEE_WALLET_HOME_DIR"),
    "dev", "balance", "--account", label,
  ], { encoding: "utf-8" });
  const line = out.split("\n").find((l) => l.startsWith("balance"));
  if (!line) throw new Error(`auction-cli dev balance printed no balance line:\n${out}`);
  return BigInt(line.trim().split(/\s+/).pop());
}

async function byName(app, objectName) {
  const res = await app.findByProperty("objectName", objectName);
  if (res.error || !res.matches || res.matches.length === 0) throw new Error(`no item named ${objectName}`);
  return res.matches[0].id;
}

async function prop(app, objectName, name) {
  const res = await app.getProperties(await byName(app, objectName));
  if (res.error) throw new Error(`getProperties(${objectName}): ${res.error}`);
  const found = res.properties.find((p) => p.name === name);
  if (!found) throw new Error(`${objectName} has no property ${name}`);
  return found.value;
}

async function setText(app, objectName, text) {
  const res = await app.inspector.send("setProperty", {
    objectId: await byName(app, objectName), property: "text", value: text,
  });
  if (res.error || res.value !== text) throw new Error(`could not set ${objectName}.text: ${res.error ?? res.value}`);
}

async function clickItem(app, objectName) {
  const res = await app.inspector.send("click", { objectId: await byName(app, objectName) });
  if (res.error) throw new Error(`click(${objectName}): ${res.error}`);
}

test("skeleton: Dev tools token transfer goes through lez_core and lands on chain", async (app) => {
  // Dev tools appears once configure and openWallet succeeded on a verified dev network.
  await app.waitFor(async () => {
    if ((await prop(app, "tabDev", "visible")) !== true) throw new Error("Dev tools hidden");
  }, { timeout: APP_READY_MS, interval: POLL_MS, description: "Dev tools tab visible" });

  const shot = process.env.SKELETON_SCREENSHOT;
  if (shot) {
    const res = await app.screenshot();
    if (res.error || !res.image) throw new Error(`screenshot: ${res.error ?? "no image"}`);
    writeFileSync(shot, Buffer.from(res.image, "base64"));
  }

  await clickItem(app, "tabDev");
  await setText(app, "devTransferFrom", FROM);
  await setText(app, "devTransferTo", TO);
  await setText(app, "devTransferAmount", AMOUNT.toString());

  const before = cliBalance(TO);
  if ((await prop(app, "devTransferSubmit", "enabled")) !== true) throw new Error("Transfer button disabled");
  await clickItem(app, "devTransferSubmit");

  let status = "";
  const start = Date.now();
  while (Date.now() - start < DONE_MS) {
    status = String(await prop(app, "devTransferStatus", "text"));
    if (status === "Done") break;
    if (status !== "" && !IN_PROGRESS.some((re) => re.test(status))) {
      throw new Error(`transfer failed: ${status}`);
    }
    await new Promise((r) => setTimeout(r, POLL_MS));
  }
  if (status !== "Done") throw new Error(`no "Done" within ${DONE_MS / 1000} s; last status: ${status}`);

  const after = cliBalance(TO);
  console.log(`    ${TO} balance: ${before} -> ${after}`);
  if (after !== before + AMOUNT) throw new Error(`${TO} balance went ${before} -> ${after}, expected +${AMOUNT}`);
});

run();
