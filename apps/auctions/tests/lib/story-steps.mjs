// The demo story as data: one step object per thing a person does in the Auctions app, in story order.
// Each Phase C task appends its segment; Task 16 writes the runner (story.mjs) that drives these through
// the QML inspector and records the timeline, and assembles the rest of the story (FULL_STORY, R39). Nothing
// here runs on its own.
//
// Step fields:
//   id        stable step name (timeline key)
//   segment   the task that added it
//   caption   what the viewer is told
//   action    "click" | "setText" | "waitFor"
//   target    objectName of the item acted on (every interactive item has one)
//   value     setText: the text; waitFor: the expected value of `property`
//   property  waitFor: the property of `target` to watch (default "text")
//   timeoutMs waitFor: how long to wait (default the runner's)
//
// A target or value may hold a {{name}} placeholder for a value only known at run time (an auction id); the
// runner fills it from what earlier steps captured.

// The position the story liquidates first (P0) and the auction that liquidation opens; ids come from the run.
export const POSITION_P0 = "{{position:p0}}";
export const AUCTION_P0 = "{{auction:p0}}";
// The position liquidated after the price move (P2) and its auction, which ends short of its target.
export const POSITION_P2 = "{{position:p2}}";
export const AUCTION_P2 = "{{auction:p2}}";

// One bid from the Live auctions tab: pick the auction, type the amount, place the bid, confirm, wait for the
// lane. `extra` steps run while the confirmation is open (before Confirm).
function bidSteps(name, auction, amount, caption, extra = []) {
  return [
    { id: `${name}-open-live`, segment: "task-12", caption: "Open the live auctions.",
      action: "click", target: "tabLive" },
    { id: `${name}-pick-auction`, segment: "task-12", caption: "Pick the auction: deepest discount first.",
      action: "click", target: `liveItem_${auction}` },
    { id: `${name}-amount`, segment: "task-12", caption, action: "setText", target: "bidAmount",
      value: String(amount) },
    { id: `${name}-quote`, segment: "task-12",
      caption: "The quote shows what the bid buys and the slippage floor.",
      action: "waitFor", target: "bidSubmit", property: "enabled", value: true, timeoutMs: 10000 },
    { id: `${name}-place`, segment: "task-12", caption: "Place the bid: a fresh quote opens the confirmation.",
      action: "click", target: "bidSubmit" },
    { id: `${name}-dialog`, segment: "task-12", caption: "Check the trade before it is sent.",
      action: "waitFor", target: "txDialog", property: "opened", value: true, timeoutMs: 10000 },
    ...extra,
    { id: `${name}-confirm`, segment: "task-12", caption: "Confirm: the bid is signed and sent.",
      action: "click", target: "txConfirm" },
    { id: `${name}-included`, segment: "task-12", caption: "The lane follows the bid until it is included.",
      action: "waitFor", target: "txLane", property: "state", value: "done", timeoutMs: 120000 },
  ];
}

// A partial bid: part of the target, the auction stays live.
export function bidPartial(auction, amount) {
  return bidSteps("bid-partial", auction, amount,
    `Bid ${amount}: part of what is left to raise.`);
}

// Any other bid, named by `suffix` ("-shortfall": the lot-binding bid on P2's auction after the price move, which
// buys the whole lot and raises less than the target).
export function bid(auction, amount, suffix) {
  return bidSteps(`bid${suffix}`, auction, amount, `Bid ${amount} on the auction.`);
}

// The final bid: it raises the rest of the target, so the confirmation names bid_with_settlement (its first
// fact, quoteRowValue_0) and the auction settles in the same transaction.
export function bidFinal(auction, amount) {
  return bidSteps("bid-final", auction, amount,
    `Bid ${amount}: the rest of the target.`,
    [{ id: "bid-final-settles", segment: "task-12",
       caption: "The final bid settles the auction: the instruction is bid_with_settlement.",
       action: "waitFor", target: "quoteRowValue_0", property: "text", value: "bid_with_settlement",
       timeoutMs: 5000 }]);
}

// Liquidate a position from the At-risk tab as the signing account (the keeper): pick it, check the quote,
// liquidate, confirm, wait for the lane. The keeper's reward goes to its <label>-coll holding. `suffix` keeps
// the ids unique when the story liquidates a second position ("-2" for P2).
export function liquidate(position, suffix = "") {
  return [
    { id: `liquidate${suffix}-open-risk`, segment: "task-13", caption: "Open the at-risk positions: lowest CR first.",
      action: "click", target: "tabRisk" },
    { id: `liquidate${suffix}-pick-position`, segment: "task-13",
      caption: "Pick the position: its CR is below the minimum, so it can be liquidated.",
      action: "click", target: `riskItem_${position}` },
    { id: `liquidate${suffix}-quote`, segment: "task-13",
      caption: "The quote shows the auction target, the collateral seized and the keeper's reward.",
      action: "waitFor", target: "liquidateSubmit", property: "enabled", value: true, timeoutMs: 10000 },
    { id: `liquidate${suffix}-start`, segment: "task-13", caption: "Liquidate: a fresh quote opens the confirmation.",
      action: "click", target: "liquidateSubmit" },
    { id: `liquidate${suffix}-dialog`, segment: "task-13", caption: "Check the liquidation before it is sent.",
      action: "waitFor", target: "riskTxDialog", property: "opened", value: true, timeoutMs: 10000 },
    { id: `liquidate${suffix}-confirm`, segment: "task-13", caption: "Confirm: the keeper signs and sends it.",
      action: "click", target: "riskTxConfirm" },
    { id: `liquidate${suffix}-included`, segment: "task-13", caption: "Included: the position's auction is open.",
      action: "waitFor", target: "riskTxLane", property: "state", value: "done", timeoutMs: 120000 },
    // Lists come from the scan cursor, so the new auction shows under Live auctions after a rescan.
    { id: `liquidate${suffix}-rescan`, segment: "task-13", caption: "Rescan: the scan picks up the new auction.",
      action: "click", target: "riskRescan" },
    { id: `liquidate${suffix}-rescan-done`, segment: "task-13", caption: "The scan has caught up with the chain.",
      action: "waitFor", target: "riskRescan", property: "enabled", value: true, timeoutMs: 30000 },
  ];
}

// Claim the leftover collateral of a settled auction from the Settled tab as its position's owner (the signing
// account must be the owner: the button is disabled for anyone else). The first step checks that the header
// names the owner, so a run signed as someone else fails there, not on a disabled button. Rescan next: the
// auction the final bid settled reaches the list through the scan cursor. The leftover goes to the owner's
// <label>-coll holding.
export function claimLeftover(auction) {
  return [
    { id: "claim-signed-as-owner", segment: "task-14", caption: "The owner of the position signs the claim.",
      action: "waitFor", target: "headerAccount", property: "text", value: "owner", timeoutMs: 10000 },
    { id: "claim-open-settled", segment: "task-14", caption: "Open the settled auctions.",
      action: "click", target: "tabSettled" },
    { id: "claim-rescan", segment: "task-14", caption: "Rescan: the scan picks up the auction the final bid settled.",
      action: "click", target: "settledRescan" },
    { id: "claim-rescan-done", segment: "task-14", caption: "The scan has caught up with the chain.",
      action: "waitFor", target: "settledRescan", property: "enabled", value: true, timeoutMs: 30000 },
    { id: "claim-pick-auction", segment: "task-14",
      caption: "Pick the auction: the target was raised, so collateral is left over for the owner.",
      action: "click", target: `settledItem_${auction}` },
    { id: "claim-ready", segment: "task-14",
      caption: "Signed in as the owner, the claim is open; anyone else sees who may claim.",
      action: "waitFor", target: "claimSubmit", property: "enabled", value: true, timeoutMs: 10000 },
    { id: "claim-start", segment: "task-14", caption: "Claim: a fresh read of the auction opens the confirmation.",
      action: "click", target: "claimSubmit" },
    { id: "claim-dialog", segment: "task-14", caption: "Check the claim before it is sent.",
      action: "waitFor", target: "settledTxDialog", property: "opened", value: true, timeoutMs: 10000 },
    { id: "claim-confirm", segment: "task-14", caption: "Confirm: the owner signs and sends it.",
      action: "click", target: "settledTxConfirm" },
    { id: "claim-included", segment: "task-14", caption: "Included: the leftover is in the owner's holding.",
      action: "waitFor", target: "settledTxLane", property: "state", value: "done", timeoutMs: 120000 },
    { id: "claim-claimed", segment: "task-14", caption: "The auction now shows what was claimed.",
      action: "waitFor", target: "claimClaimed", property: "visible", value: true, timeoutMs: 10000 },
  ];
}

// Sign as `label` from the header picker; the last step checks the header names it, so a run signed as
// someone else stops there, not on a disabled button.
function signAs(name, label, caption) {
  return [
    { id: `${name}-open-picker`, segment: "task-15", caption, action: "click", target: "accountPicker" },
    { id: `${name}-pick`, segment: "task-15", caption: `Sign as ${label}.`, action: "click",
      target: `accountOption_${label}` },
    { id: `${name}-signed`, segment: "task-15", caption: `The header names ${label}.`, action: "waitFor",
      target: "headerAccount", property: "text", value: label, timeoutMs: 10000 },
  ];
}

// One System transaction: open the tab and the section, wait for the button, click it, confirm, wait for the
// lane. Every System transaction shares the "system" dialog and lane (R34); a new action clears a finished
// lane before its dialog opens, so the lane wait sees this transaction. `before` steps run after the section
// opens (an amount to type).
function systemTx(name, section, button, caption, confirmCaption, before = []) {
  return [
    { id: `${name}-open-system`, segment: "task-15", caption: "Open System.", action: "click", target: "tabSystem" },
    { id: `${name}-section`, segment: "task-15", caption, action: "click", target: `systemItem_${section}` },
    ...before,
    { id: `${name}-ready`, segment: "task-15", caption: "The figures are read; the action is open.",
      action: "waitFor", target: button, property: "enabled", value: true, timeoutMs: 10000 },
    { id: `${name}-start`, segment: "task-15", caption: "A fresh read opens the confirmation.",
      action: "click", target: button },
    { id: `${name}-dialog`, segment: "task-15", caption: "Check it before it is sent.",
      action: "waitFor", target: "systemTxDialog", property: "opened", value: true, timeoutMs: 10000 },
    { id: `${name}-confirm`, segment: "task-15", caption: confirmCaption, action: "click", target: "systemTxConfirm" },
    { id: `${name}-included`, segment: "task-15", caption: "Included: the figures are read again.",
      action: "waitFor", target: "systemTxLane", property: "state", value: "done", timeoutMs: 120000 },
  ];
}

// The System builders. Each takes an id suffix (default none) so one step can appear twice in the story without
// colliding ids: the demo settles debt and checks solvency more than once (e.g. settleDebt("-2")).

// Upkeep as the caller (permissionless): record the feed, settle the buffer against unbacked debt.
export function observeFeed(suffix = "") {
  return systemTx(`system-observe-feed${suffix}`, "accounting", "observeFeed",
    "Accounting: upkeep anyone may run.", "Observe the feed: it records the feed's state, no price moves.");
}

export function settleDebt(suffix = "") {
  return systemTx(`system-settle-debt${suffix}`, "accounting", "settleDebtSubmit",
    "Accounting: the buffer covers unbacked debt.",
    "Settle debt: the buffer's stable is burned against the debt.");
}

// Settle debt when there is nothing to settle (after F1–F3 the target was raised, so no debt is unbacked): the quote
// says 0 and the button stays disabled, so the story shows it rather than sending a no-op transaction (the CLI demo's
// `settle-debt` at this point changes nothing either).
export function settleDebtNothingToSettle(suffix = "") {
  const name = `system-settle-debt-nothing${suffix}`;
  return [
    { id: `${name}-open-system`, segment: "task-16", caption: "Open System.", action: "click", target: "tabSystem" },
    { id: `${name}-section`, segment: "task-16", caption: "Accounting: is any debt unbacked?", action: "click",
      target: "systemItem_accounting" },
    { id: `${name}-quote`, segment: "task-16",
      caption: "The target was raised: nothing is unbacked, so there is nothing for the buffer to settle.",
      action: "waitFor", target: "settleDebtAmount", property: "text", value: "0", timeoutMs: 10000 },
  ];
}

export function checkSolvency(suffix = "") {
  return systemTx(`system-check-solvency${suffix}`, "accounting", "checkSolvency",
    "Accounting: is unbacked debt above the freeze threshold?", "Check solvency.");
}

export function startSurplus(suffix = "") {
  return systemTx(`system-start-surplus${suffix}`, "surplus", "startSurplus",
    "Surplus auction: the buffer has more than it needs.", "Start the surplus auction.");
}

export function startDebt(suffix = "") {
  return systemTx(`system-start-debt${suffix}`, "debt", "startDebt",
    "Debt auction: unbacked debt is put up for protocol tokens.", "Start the debt auction.");
}

// Take a lot as the signing account (the taker): type the stable amount, wait for its quote, confirm.
export function takeSurplus(amount, suffix = "") {
  const name = `system-take-surplus${suffix}`;
  return systemTx(name, "surplus", "takeSurplusSubmit",
    "Surplus auction: take the lot with protocol tokens.",
    "Confirm: the protocol tokens paid are burned.",
    [{ id: `${name}-amount`, segment: "task-15", caption: `Take ${amount} stable.`,
       action: "setText", target: "takeSurplusAmount", value: String(amount) },
     { id: `${name}-quote`, segment: "task-15", caption: "The quote names the burn.",
       action: "waitFor", target: "takeSurplusBurnNote", property: "visible", value: true, timeoutMs: 10000 }]);
}

export function takeDebt(amount, suffix = "") {
  const name = `system-take-debt${suffix}`;
  return systemTx(name, "debt", "takeDebtSubmit",
    "Debt auction: pay down bad debt for newly minted protocol tokens.",
    "Confirm: the stable paid is burned against the debt.",
    [{ id: `${name}-amount`, segment: "task-15", caption: `Pay ${amount} stable.`,
       action: "setText", target: "takeDebtAmount", value: String(amount) },
     { id: `${name}-quote`, segment: "task-15", caption: "The quote shows the tokens minted.",
       action: "waitFor", target: "takeDebtNote", property: "visible", value: true, timeoutMs: 10000 }]);
}

// Exported for Task 16 (story.mjs): sign as a label from the header picker, with the account guard.
export { signAs };

export const STORY_STEPS = [
  // ---- Task 11: shell. Pick the account that signs; the header follows. ----
  {
    id: "shell-open-picker",
    segment: "task-11",
    caption: "One person plays every role: the signing account is one click away in the header.",
    action: "click",
    target: "accountPicker",
  },
  {
    id: "shell-pick-keeper",
    segment: "task-11",
    caption: "Sign as the keeper.",
    action: "click",
    target: "accountOption_keeper",
  },
  {
    id: "shell-header-shows-keeper",
    segment: "task-11",
    caption: "The header now names the keeper; balances are read again, no funds move.",
    action: "waitFor",
    target: "headerAccount",
    property: "text",
    value: "keeper",
    timeoutMs: 10000,
  },

  // ---- Task 13: At-risk positions. The keeper liquidates P0, which opens P0's auction. ----
  ...liquidate(POSITION_P0),

  // ---- Task 12: Live auctions. The owner bids on P0's auction: a partial bid, then the final one. ----
  {
    id: "live-open-picker",
    segment: "task-12",
    caption: "The owner bids: switch the signing account.",
    action: "click",
    target: "accountPicker",
  },
  {
    id: "live-pick-owner",
    segment: "task-12",
    caption: "Sign as the owner.",
    action: "click",
    target: "accountOption_owner",
  },
  {
    id: "live-header-shows-owner",
    segment: "task-12",
    caption: "The header names the owner.",
    action: "waitFor",
    target: "headerAccount",
    property: "text",
    value: "owner",
    timeoutMs: 10000,
  },
  ...bidPartial(AUCTION_P0, 100000),
  ...bidFinal(AUCTION_P0, 230000),

  // ---- Task 14: Settled. Still signed in as the owner, claim what the final bid left over. ----
  ...claimLeftover(AUCTION_P0),

  // ---- Task 15: System. Upkeep as the caller, then the surplus take as the taker. ----
  // The debt side is not here (R39): startDebt() and takeDebt(5000) only make sense after the price publish,
  // the P2 liquidation, its bid, a second settleDebt("-2") and a stable transfer to the taker, in the order of
  // scripts/lib/demo-scenario.sh. Task 16 assembles them, with signAs(), from the exported builders.
  ...signAs("system-caller", "caller", "Upkeep is permissionless: sign as the caller."),
  ...observeFeed(),
  ...settleDebtNothingToSettle(),
  ...checkSolvency(),
  ...startSurplus(),
  ...signAs("system-taker", "taker", "The taker pays for the surplus lot."),
  ...takeSurplus(500),
];

// ---- Task 16: Dev tools. They replace the demo's terminal steps (scripts/lib/demo-scenario.sh). ----

// The price move (dp_price_move): publish `price` (collateral per 1 stable, typed as a decimal) from Dev tools >
// Oracle price. Signed by oracle-src, the active account as host admin, and engine-admin: three transactions,
// so the lane waits for all three (R27).
export function publishPrice(price, suffix = "") {
  const name = `dev-publish-price${suffix}`;
  return [
    { id: `${name}-open-dev`, segment: "task-16", caption: "Open Dev tools: only on a verified dev network.",
      action: "click", target: "tabDev" },
    { id: `${name}-section`, segment: "task-16", caption: "Oracle price: move the market price.",
      action: "click", target: "devItem_price" },
    { id: `${name}-amount`, segment: "task-16", caption: `New price ${price}: inside ½ to 2 × the redemption price.`,
      action: "setText", target: "devPrice", value: String(price) },
    { id: `${name}-quoted`, segment: "task-16", caption: "The engine's reading at the new price: CR is judged at R, so no CR moves.",
      action: "waitFor", target: "devAffectedTitle", property: "text", value: `At ${price} collateral per stable`,
      timeoutMs: 15000 },
    { id: `${name}-quote`, segment: "task-16",
      caption: "The engine checks the price on its own feed path before anything is signed.",
      action: "waitFor", target: "devPriceVerdict", property: "text", value: "The engine accepts this price now.",
      timeoutMs: 15000 },
    { id: `${name}-ready`, segment: "task-16", caption: "Publish is open.",
      action: "waitFor", target: "devPricePublish", property: "enabled", value: true, timeoutMs: 10000 },
    { id: `${name}-start`, segment: "task-16", caption: "Publish: a fresh quote opens the confirmation.",
      action: "click", target: "devPricePublish" },
    { id: `${name}-dialog`, segment: "task-16", caption: "Three signers, three transactions.",
      action: "waitFor", target: "devTxDialog", property: "opened", value: true, timeoutMs: 10000 },
    { id: `${name}-confirm`, segment: "task-16", caption: "Confirm: the price account, the host rotation, the engine follow-up.",
      action: "click", target: "devTxConfirm" },
    { id: `${name}-included`, segment: "task-16", caption: "All three steps included: the new price is live.",
      action: "waitFor", target: "devTxLane", property: "state", value: "done", timeoutMs: 300000 },
  ];
}

// The stable transfer before the debt take (dp_f4): Dev tools > Token transfer, its own status line (Task 5).
export function tokenTransfer(from, to, amount, suffix = "") {
  const name = `dev-transfer${suffix}`;
  return [
    { id: `${name}-open-dev`, segment: "task-16", caption: "Open Dev tools.", action: "click", target: "tabDev" },
    { id: `${name}-section`, segment: "task-16", caption: "Token transfer: fund the taker.",
      action: "click", target: "devItem_transfer" },
    { id: `${name}-from`, segment: "task-16", caption: `From ${from}.`, action: "setText", target: "devTransferFrom",
      value: from },
    { id: `${name}-to`, segment: "task-16", caption: `To ${to}.`, action: "setText", target: "devTransferTo", value: to },
    { id: `${name}-amount`, segment: "task-16", caption: `${amount} stable.`, action: "setText",
      target: "devTransferAmount", value: String(amount) },
    { id: `${name}-submit`, segment: "task-16", caption: "Transfer.", action: "click", target: "devTransferSubmit" },
    { id: `${name}-done`, segment: "task-16", caption: "Included: the taker can pay for a debt lot.",
      action: "waitFor", target: "devTransferStatus", property: "text", value: "Done", timeoutMs: 120000 },
  ];
}

// The freeze trigger (dp_freeze): Dev tools > Protocol token, signed by engine-admin; the dialog repeats the
// warning before anything is sent.
export function disableProtocolToken(suffix = "") {
  const name = `dev-disable-token${suffix}`;
  return [
    { id: `${name}-open-dev`, segment: "task-16", caption: "Open Dev tools.", action: "click", target: "tabDev" },
    { id: `${name}-section`, segment: "task-16", caption: "Protocol token: the freeze trigger.",
      action: "click", target: "devItem_token" },
    { id: `${name}-ready`, segment: "task-16",
      caption: "Without a protocol token no debt auction can cover the bad debt left.",
      action: "waitFor", target: "devDisableProtocolToken", property: "enabled", value: true, timeoutMs: 10000 },
    { id: `${name}-start`, segment: "task-16", caption: "Disable protocol token.", action: "click",
      target: "devDisableProtocolToken" },
    { id: `${name}-dialog`, segment: "task-16", caption: "The confirmation repeats the warning.",
      action: "waitFor", target: "devTxDialog", property: "opened", value: true, timeoutMs: 10000 },
    { id: `${name}-confirm`, segment: "task-16", caption: "Confirm: engine-admin signs.", action: "click",
      target: "devTxConfirm" },
    { id: `${name}-included`, segment: "task-16", caption: "Included: the protocol token is off.",
      action: "waitFor", target: "devTxLane", property: "state", value: "done", timeoutMs: 120000 },
  ];
}

// The story after the surplus take, in the order of scripts/lib/demo-scenario.sh (R39): the price move (1.9 R),
// P2 liquidated and its lot-binding bid (the shortfall), the buffer settles what it can and a debt auction
// starts, the taker is funded and takes one lot, then the freeze trigger and the solvency check that freezes
// the host. Every id is unique (story.mjs refuses a story with a repeated id).
export const STORY_STEPS_AFTER_SURPLUS = [
  ...signAs("dev-admin", "admin", "The admin moves the price: sign as admin (host admin)."),
  ...publishPrice("1.9"),
  ...signAs("shortfall-keeper", "keeper", "The keeper liquidates P2 at the new price."),
  ...liquidate(POSITION_P2, "-2"),
  ...signAs("shortfall-owner", "owner", "The owner bids on P2's auction."),
  ...bid(AUCTION_P2, 330000, "-shortfall"),
  ...signAs("debt-caller", "caller", "Upkeep: sign as the caller."),
  ...settleDebt("-2"),
  ...startDebt(),
  ...tokenTransfer("owner-stable", "taker-stable", 5000),
  ...signAs("debt-taker", "taker", "The taker pays down a lot of bad debt."),
  ...takeDebt(5000),
  ...disableProtocolToken(),
  ...signAs("freeze-caller", "caller", "Anyone may check solvency: sign as the caller."),
  ...checkSolvency("-2"),
  { id: "story-frozen", segment: "task-16", caption: "The host is frozen: new debt is refused.",
    action: "waitFor", target: "headerFrozen", property: "visible", value: true, timeoutMs: 20000 },
];

export const FULL_STORY = [...STORY_STEPS, ...STORY_STEPS_AFTER_SURPLUS];
