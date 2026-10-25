// The demo story as data: one step object per thing a person does in the Auctions app, in story order.
// Each Phase C task appends its segment; Task 16 writes the runner (story.mjs) that drives these through
// the QML inspector and records the timeline. Nothing here runs on its own.
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
// liquidate, confirm, wait for the lane. The keeper's reward goes to its <label>-coll holding.
export function liquidate(position) {
  return [
    { id: "liquidate-open-risk", segment: "task-13", caption: "Open the at-risk positions: lowest CR first.",
      action: "click", target: "tabRisk" },
    { id: "liquidate-pick-position", segment: "task-13",
      caption: "Pick the position: its CR is below the minimum, so it can be liquidated.",
      action: "click", target: `riskItem_${position}` },
    { id: "liquidate-quote", segment: "task-13",
      caption: "The quote shows the auction target, the collateral seized and the keeper's reward.",
      action: "waitFor", target: "liquidateSubmit", property: "enabled", value: true, timeoutMs: 10000 },
    { id: "liquidate-start", segment: "task-13", caption: "Liquidate: a fresh quote opens the confirmation.",
      action: "click", target: "liquidateSubmit" },
    { id: "liquidate-dialog", segment: "task-13", caption: "Check the liquidation before it is sent.",
      action: "waitFor", target: "txDialog", property: "opened", value: true, timeoutMs: 10000 },
    { id: "liquidate-confirm", segment: "task-13", caption: "Confirm: the keeper signs and sends it.",
      action: "click", target: "txConfirm" },
    { id: "liquidate-included", segment: "task-13", caption: "Included: the position's auction is open.",
      action: "waitFor", target: "txLane", property: "state", value: "done", timeoutMs: 120000 },
    // Lists come from the scan cursor, so the new auction shows under Live auctions after a rescan.
    { id: "liquidate-rescan", segment: "task-13", caption: "Rescan: the scan picks up the new auction.",
      action: "click", target: "riskRescan" },
    { id: "liquidate-rescan-done", segment: "task-13", caption: "The scan has caught up with the chain.",
      action: "waitFor", target: "riskRescan", property: "enabled", value: true, timeoutMs: 30000 },
  ];
}

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
];
