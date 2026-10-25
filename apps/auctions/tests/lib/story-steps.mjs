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
];
