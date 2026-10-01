# Auctions mockup: v2 review and v3 rebuild

- v2: `companion/.superpowers/brainstorm/469-1790873518/content/app-layout-v2.html`
- v3: `companion/.superpowers/brainstorm/469-1790873518/content/app-layout-v3.html` (full HTML document)
- Render: `scratchpad/ui-v3.png` (headless Chrome, 1260×1240: the Basecamp window plus a bid-card states strip)
- v3 build inputs: `scratchpad/v3-template.html` + `scratchpad/gen_v3.py`. The generator computes the chart geometry and writes v3. Re-run `python gen_v3.py` after you edit the template.

## Why v2 "looks off"

The main problem is that v2 copied the Dribbble shot's skin instead of Basecamp's. It reads as a light SaaS page dropped into a dark Qt shell. Specifics:

| Before (v2) | After (v3) | Why |
| --- | --- | --- |
| Light theme (`#f7f7f9` page, white cards, `box-shadow`) | `#171717` page, `#232323` surfaceRaised panel, `#171717` inset cards with a `#333333` border | Basecamp and Logos.Theme are dark-only. `Theme.availableThemes` has just `"dark"`. |
| Purple accent `#7b6cf6`, plus green, orange and red from the Flat UI palette | One accent, `accentOrange`/`primary` `#ED7B58`. Status colours are the palette's success, warning and error. | v2 invented a brand. In v2, orange meant "now", green meant "elapsed" and purple meant "schedule", and none of them were explained. |
| System font at 10–12px throughout. 20px page title, 16px card title. | Public Sans with the Typography scale: 36 page title, 24 panel title and KPI values, 16 list amounts, 14 body, 12 captions | v2 had no hierarchy: every number was the same bold 12px. The scale is now the one AppManagerView and SettingsView use. |
| No tabular figures | `font-variant-numeric: tabular-nums` everywhere (QML: `font.features: {"tnum": 1}`) | The countdown and the live figures would jitter as they tick. |
| Ad-hoc layout: 14px padding, 12px and 8px gaps, list at 32% width | AppManagerView grid: 40 margins, 20 between header and body, 200px left column, 20 gutter, panel with 16 margins and 12 spacing | The view should be a sibling of Applications and Settings, not a one-off. |
| Tabs were a white pill and grey text | LogosTabBar/LogosTabButton: 14 medium, `textTertiary` → `primary`, 3px underline on a hairline track, in the header's right slot (where AppManagerView puts its search bar) | Matches the design-system component and AppManagerView's tab usage. |
| Selected list item used a 2px purple border while the others had 1px | LogosItemDelegate highlight (`backgroundButton` `#2F2F2F`, radiusLarge) plus the 3px `accentOrange` bar from SidebarAppDelegate | The border-width change shifted the layout by 1px, and the colour was off-brand. |
| List progress bar was unlabelled (raised? time?) | `81% raised` label. Rows sorted by "ending soonest". #40 reads "Ending · 3m 58s" in warning yellow. | v2 signalled urgency with red text only, and the bar meant nothing without a label. |
| Chart: a 220×110 viewBox stretched to the column width, so the 8px labels scaled unpredictably | Fixed-pixel SVG (424×206) with labelled axes (0–10%, 0–2h), dotted gridlines, a max-10% reference, an expiry line, a knee marker "Max at 14:47", a filled elapsed segment, a dashed schedule ahead, and a "Now 6.2%" pin | v2's chart was decoration. The 45m tick sat at 63% of the axis instead of 37.5%. |
| v2's "now" point sat past 45m but below 10%, which is impossible on a schedule that holds at 10% after 45m | Now = 23m36s elapsed for 6.2% (see "Data consistency") | The chart has to agree with the numbers. |
| Stats strip: four equal bold 12px values | KPI card: discount (accent, with a "+0.18% per min" rate), time left (with end time), raised/target (with a bar), collateral left (of 12.50) | Each KPI answers one bidder question: how good is the deal, how fast is it improving, how much is left, how long remains. |
| Oracle row in the spec table, far from the bid button | Oracle price, freshness bar (2m of 60m) and a FRESH/STALE badge at the top of the bid card | A stale feed pauses bidding, so the bidder needs to see that where they act. |
| Bid input text "Pay LSC (minimum 25 · max 5,230 to finish)", with the preview and private toggle crammed into one 11px line | LogosTextField with a balance. Hints row "Min 25" plus a "Max to finish 5,230" link that fills the field. "You get ≈ 0.802 wTKA" at 24px, "At least 0.798 with 0.5% slippage" with an Edit link. LogosSwitch for "Private bid: Deshield → bid → reshield". Full-width Primary button. | The "what exactly will I get" answer is now the biggest thing in the bid card. |
| White label on the `#ED7B58` button (contrast 2.8:1) | Black label (contrast 7.6:1) | 2.8:1 fails WCAG AA. Basecamp already puts black glyphs on accentOrange (SidebarCircleButton.qml:27,37). LogosButton hard-codes `palette.text` for its label, so building this needs either a `contentItem` override or a DS fix (`primary` buttons should use `backgroundBlack` text). |
| No states | A states strip with "Oracle stale" (STALE badge, warning freshness bar, LogosNotice Warning "Bidding paused", disabled button) and "Over the max" (error field, "Only 5,230 LSC is left to raise. Use 5,230", preview at the capped amount) | These are the two moments where a bidder can act on wrong information. |
| Basecamp context was a grey "Basecamp › Auctions" bar | The real shell: SidebarPanel (80px, `#262626`, radius 16, real basecamp/tent/dashboard/module/settings icons embedded from `src/Basecamp/Icons`), the WorkspaceArea tab strip (20px tabs, radius 10 top, `#262626`/`#A4A4A4` selected, `#626262` idle), the 4px DockCard gap and the 1100×720 plugin card | The screen now reads in context. |

## Data consistency (please confirm)

The example numbers in the brief contradict the schedule (2% → 10% linearly over 45m, then hold to 2h):

- **#41**: 6.2% means 23m36s elapsed, so **1h 36m 24s left**. The brief's "31m 12s left" would be 88m elapsed, where the discount must be 10%. v3 keeps **6.2%** because it drives the 0.802 wTKA preview, which checks out exactly: 1000 × 0.000752 / (1 − 0.062) = 0.8017, and ×0.995 = 0.798. v3 shows 1h 36m. Max discount is reached at 14:47 and the auction ends at 16:02 (started 14:02).
- **#40**: "9.8% with 4m left" is impossible because the discount is 10% after 45m. v3 shows **10.0%** with 3m 58s left.
- **#42**: 2.4% means about 2m elapsed, so **1h 57m** left, not 1h 52m.
- Not changed: 4,120 LSC raised against 5.49 wTKA sold implies a price far below oracle. That's fine for a mockup, but it won't survive real data.

If the 31m figure was the intended value, the alternative is to show #41 holding at 10% with a flat "now". The chart and the "+0.18%/min" rate would then become "At max, holds until 16:02".

## Tokens used (CSS custom property → source)

LDS = `scratchpad/repos/logos-design-system/src/qml/Logos/Theme/` @ 88330d0. BC = `logos-basecamp/src/`.

### Colour
| CSS var | Value | Logos token | Defined | Used by Basecamp at |
| --- | --- | --- | --- | --- |
| `--background` | `#171717` | `palette.background` = gray900 | LDS DarkTheme.qml:6, ColorPalette.qml:39 | BC MainContainer.cpp:155 (window bg), WorkspaceArea.cpp:56 (DockCard corner cut), :182, :550; Basecamp/AppManager/AppManagerView.qml:64; Basecamp/Settings/SettingsView.qml:43; RepositoriesView.qml:128 (inset row) |
| `--background-secondary` | `#262626` | `palette.backgroundSecondary` = gray850 | DarkTheme.qml:7, ColorPalette.qml:36 | Basecamp/Sidebar/SidebarPanel.qml:54; WorkspaceArea.cpp:556 (selected tab); LogosTextField bg |
| `--background-button` | `#2F2F2F` | `palette.backgroundButton` = gray340 | DarkTheme.qml:13, ColorPalette.qml:24 | AppManagerView.qml:156, SettingsView.qml:104 (selected list item) |
| `--surface-raised` | `#232323` | `palette.surfaceRaised` = gray360 | DarkTheme.qml:15, ColorPalette.qml:27 | AppManagerView.qml:172, SettingsView.qml:120 (main panel); ConfirmationDialog.qml:192 |
| `--surface` | `#343434` | `palette.surface` = gray320 | DarkTheme.qml:14, ColorPalette.qml:22 | Basecamp/Sidebar/SidebarCircleButton.qml:27 |
| `--text` | `#FFFFFF` | `palette.text` | DarkTheme.qml:20 | AppManagerView.qml:84 |
| `--text-secondary` | `#A4A4A4` | `palette.textSecondary` = gray400 | DarkTheme.qml:21, ColorPalette.qml:30 | AppManagerView.qml:90 (subtitle); WorkspaceArea.cpp:557 |
| `--text-tertiary` | `#969696` | `palette.textTertiary` = gray500 | DarkTheme.qml:23, ColorPalette.qml:31 | AppManagerView.qml:160, PackageRowDelegate.qml:104 |
| `--text-tab-idle` | `#626262` | (none, raw) | n/a | BC WorkspaceArea.cpp:570 |
| `--border` | `#434343` | `palette.border` = gray300 | DarkTheme.qml:27, ColorPalette.qml:21 | Basecamp/AppManager/AppTile.qml:40 |
| `--border-subtle` | `#333333` | `palette.borderSubtle` = gray330 | DarkTheme.qml:31, ColorPalette.qml:23 | AppManagerView.qml:282 (panel divider); RepositoriesView.qml:129 (inset row border); PackageRowDelegate.qml:49 |
| `--border-hairline` | `rgba(128,128,128,.2)` | `palette.borderHairline` | DarkTheme.qml:32 | LogosTabBar track |
| `--border-tertiary-muted` | `rgba(150,150,150,.2)` | `palette.borderTertiaryMuted` | DarkTheme.qml:30 | SidebarPanel.qml:209 (separator) |
| `--accent` | `#ED7B58` | `palette.accentOrange` = `palette.primary` = orange300 | DarkTheme.qml:38, :61; ColorPalette.qml:48 | SidebarAppDelegate.qml:46 (selected bar); SidebarCircleButton.qml:27; RepositoriesView.qml:153; AppGridDelegate.qml:102 |
| `--accent-hover` | `#F55702` | `palette.primaryHover` = orange500 | DarkTheme.qml:39, ColorPalette.qml:54 | LogosButton hover |
| `--accent-border` | `rgba(237,123,88,.4)` | `palette.primaryBorder` | DarkTheme.qml:58 | (chart "now" pin) |
| `--accent-wash` | accent @ 0.18 | LogosBadge bg `getColor(color, 0.18)` | LDS Controls/LogosBadge.qml:14 | badges |
| `--black` | `#000000` | `palette.backgroundBlack` | DarkTheme.qml:11 | SidebarCircleButton.qml:37 (glyph on accent) |
| `--success` | `#49F563` | `palette.success` = green500 | DarkTheme.qml:43, ColorPalette.qml:77 | FRESH and "Bidding open" badges, oracle bar |
| `--warning` / `--warning-border` | `#FEBC2E` / @0.4 | `palette.warning` / `warningBorder` | DarkTheme.qml:53, :55; ColorPalette.qml:70 | AppGridDelegate.qml:97, PackageRowDelegate.qml:142 |
| `--error` | `#FB3748` | `palette.error` = red500 | DarkTheme.qml:48, ColorPalette.qml:64 | RepositoriesView.qml:83-84 |
| `--notice-bg` | `#0E121B` | `palette.backgroundElevated` = gray950 | DarkTheme.qml:9, ColorPalette.qml:40 | LogosNotice bg (stale state) |
| `--focus` | `#FF8800` | `palette.focus` = orange400 | DarkTheme.qml:70, ColorPalette.qml:51 | focus ring; LogosTextField focus border is `overlayOrange` (orange400 @ 0.3, DarkTheme.qml:76) |

Sidebar app tiles use `AppColors.colorForApp()` = `Qt.hsla(hash-hue, 0.38, 0.22)` (BC Basecamp/AppManager/AppColors.qml:19-26). The hues in the mockup are illustrative because the real ones come from hashing each app name.

Legacy hex in older Basecamp views was deliberately **not** used: ModulesView.qml:34-36 (`#238636` green button), CoreModulesView.qml:40-79 and UiModulesTab.qml:23-73 (`#2d2d2d`, `#3d3d3d`, `#a0a0a0`, 20px titles), ModuleRow.qml:38-39 (`#363636`/`#2d2d2d` zebra rows, radius 6), ConfirmationDialog.qml:329-439 (`#1e1e1e`, `#e0e0e0`, 13px). Those predate Logos.Theme. The current views (AppManagerView, SettingsView, RepositoriesView, Sidebar) use tokens, and v3 follows those.

### Spacing, radius, size
| CSS var | Value | Token | Defined | Basecamp usage mirrored |
| --- | --- | --- | --- | --- |
| `--tiny` … `--xxlarge` | 4, 8, 12, 16, 20, 40 | `spacing.tiny`…`xxlarge` | LDS Spacing.qml:5-10 | Page margins 40 (AppManagerView.qml:68); header→body 20 (:69); body gutter 20 (:124); left column item spacing 4 (:132, :147); panel margins 16 (:184); panel spacing 12 (:185); panel header spacing 20 (:191); tabs spacing 16 (:205) |
| `--r-small` / `--r-medium` / `--r-large` / `--r-xlarge` | 4 / 6 / 8 / 16 | `spacing.radiusSmall`…`radiusXlarge` | Spacing.qml:14-17 | Panel 16 (AppManagerView.qml:173); list item and button 8 (:155, :239); app tile 6 (AppTile.qml:18); text field 4 (LogosTextField) |
| Left column width | 200px | (none) | n/a | AppManagerView.qml:128-130, SettingsView.qml:76-78 |
| Button | 40px high, radiusLarge | (none) | n/a | AppManagerView.qml:238-239 (LogosButton default is 44px, radiusXlarge) |
| Sidebar | 80px; app rows 50px; tiles 38px; circle buttons 38px; spacing 16/12/2 | (none) | n/a | MainContainer.cpp:175-176; SidebarAppDelegate.qml:20, :25; SidebarCircleButton.qml:9-10; SidebarPanel.qml:58, :117, :172 |
| Workspace tab strip | 20px high, min-width 120, radius 10 top, padding 0 8 0 4, margin-right 10 | (none) | n/a | WorkspaceArea.cpp:555-571 |
| Shell margins | window 4,0,4,2; content 4,9,4,4; DockCard top gap 4, corner radius 16 | (none) | n/a | MainContainer.cpp:165, :184; WorkspaceArea.cpp:57, :100 |

### Typography
| CSS var | Value | Token | Defined | Basecamp usage mirrored |
| --- | --- | --- | --- | --- |
| `--font` | Public Sans 400/500/700 (Google Fonts in the mockup; the app bundles the TTFs) | `typography.publicSans` | LDS Typography.qml:5-20 | all |
| `--page-title` | 36 bold | `pageTitleText` | Typography.qml:32 | AppManagerView.qml:82-83, SettingsView.qml:57-58 |
| `--panel-title` | 24 (panel title medium; KPI and "you get" values bold) | `panelTitleText` | Typography.qml:34 | AppManagerView.qml:196-197, RepositoriesView.qml:56-57 |
| `--subtitle` | 16 (left-column heading regular) | `subtitleText` | Typography.qml:35 | AppManagerView.qml:138-139 |
| `--primary` | 14 | `primaryText` | Typography.qml:36 | AppManagerView.qml:89 (subtitle), LogosTabButton |
| `--secondary` | 12 | `secondaryText` | Typography.qml:37 | captions, LogosButton label, LogosTextField |
| `--badge` | 11 uppercase, letter-spacing 0.22 | LogosBadge literal | LDS Controls/LogosBadge.qml:58-61 | badges |
| `--tiny-text` | 8 | `badgeText` | Typography.qml:38 | SidebarPanel.qml:198 (version) |
| `--mono` | platform monospace | `typography.mono` | Typography.qml (mono) | ids and addresses |

## Key design decisions

1. **The page is a sibling of Applications and Settings.** It has the same header (36/14), the same 200px left column of LogosItemDelegate rows, and the same surfaceRaised radius-16 panel with a 24px medium title and a borderSubtle divider. Inset cards copy RepositoriesView rows (background, borderSubtle, radiusLarge).
2. **The view-level tabs go in the header's right slot**, where AppManagerView puts its search bar, because they change the left list and the panel together.
3. **One accent with one meaning.** Orange marks "now / this auction / act": the current discount, the chart's now-pin and elapsed fill, the selected-row bar, the active tab and the primary button. Green, yellow and red are reserved for oracle and status verdicts.
4. **The discount chart is the signature element.** It is a real chart (axes, gridlines, max reference, knee, expiry, an elapsed segment drawn solid and the schedule ahead dashed), and in QML it is a cheap `QtQuick.Shapes` build.
5. **The bid card answers "what do I get for X" first.** The oracle verdict sits on top (it can block bidding), then the amount with Min and a one-click "Max to finish", then a 24px "You get" with the guaranteed floor, then the private-bid switch and the button.
6. **No effects QML can't do cheaply.** There are no shadows, blur or gradients: just Rectangles, borders, a Shape and a 160ms press scale.

---

# v4: At-risk positions + Bid confirmation

- Output: `companion/.superpowers/brainstorm/26491-1790875357/content/screens-v4.html` (full HTML document)
- Render: `scratchpad/ui-v4.png` (headless Chrome, 1260×2580)
- Build inputs: `scratchpad/v4-screens-template.html` + `scratchpad/gen_v4.py`. The generator pulls v3's CSS, sidebar and whole plugin view (with the chart) out of `v3-template.html`, so the shell cannot drift from the approved render. It also draws the `LogosStageLane` chevrons with the same geometry as `LogosStageLane.qml` (notch 14, gap 2, corner 3, quadratic rounding). Run `python gen_v4.py` after editing the template.
- `v4-template.html` (the interrupted partial) is left untouched. What was kept from it: the list row layout, the gauge, the quote card skeleton, the four state cards, and the two-list privacy summary. What changed is listed below.

## Corrections applied everywhere
- A stale oracle **or** a market price outside [R/2, 2R] pauses that collateral type. There is no fallback price. The quote card shows "wTKA paused · 12m" (feed 72m old, max 60m, so paused 12m) or "Price feed out of range" (0.001610 vs 0.000376–0.001504), with the button disabled as "Liquidations paused".
- While paused, the discount clock is frozen. The live-auction row shows "Paused 12m" with the discount greyed. The notice reads "The discount is frozen at 6.2% until the feed recovers. The clock is stopped too: 1h 36m stays left." This **replaces v3's "Oracle stale" state**, whose copy ("The discount clock keeps running") is now wrong. v3's render itself was not edited.

## Screen 1 decisions
1. **Same shell, same grid.** The header, tabs (At-risk positions active), 200px list and surfaceRaised panel are byte-identical to v3.
2. **The list is sorted lowest CR first and filtered to CR < 160%.** The filter explains why the healthy 152.0% row is there. The tab count (2) counts only liquidatable rows. "Scanned to block 872 · Rescan" sits quietly under the list heading, and the footer note says there is no indexer yet.
3. **Position id changed to `d4…07`.** v3 already uses `7c…19` as the position behind auction #41 (8,500 LSC debt), so reusing it here would contradict v3.
4. **CR is the hero.** It is set at 30px (`titleText`) in error red, with a 100–250% gauge: a red zone below the white 150% tick, and the marker at 3.03%. Below that come the six facts the brief asks for (collateral, debt incl. fee, debt value, redemption price, market price with freshness, allowed range), then a short "When you liquidate" sequence so the keeper knows what the button sets off (auction #43 and its schedule).
5. **Quote card.** It is grouped as *what the auction must raise* (debt, penalty, target), then *where the collateral goes* (seized, your reward, to auction, fee), then *capacity*. The reward line carries its formula and "comes **out of the penalty**, so liquidating your own position never pays". This replaces the partial's "None if you own the position", which the brief contradicts.
6. **"Will end short" note (new).** With these numbers the lot cannot raise the target even at a 2% discount, and the screen says so instead of hiding it.
7. **States:** Healthy (disabled button + "CR 152.0% is above the 150% minimum."), Oracle stale, Price feed out of range, and Live auction paused.

## Screen 1 arithmetic (R = 0.000752 wTKA per LSC; collateral per stablecoin)
| Quantity | Formula | Value |
| --- | --- | --- |
| Debt | principal + accrued fee | 15,820.00 + 80.00 = **15,900.00 LSC** |
| Debt value | debt × R | 15,900 × 0.000752 = **11.9568 wTKA** (shown 11.957) |
| CR | collateral ÷ debt value | 12.50 ÷ 11.9568 = **104.543%** (104.5%) |
| Collateral in LSC | collateral ÷ R | **16,622.34 LSC** |
| Penalty | 10% × debt | **1,590.00 LSC** |
| Auction target | debt × 1.10 | **17,490.00 LSC** |
| Keeper reward | min(tip + 0.1% × debt, cap), tip = 5 LSC, cap = 50 LSC | min(5 + 15.90, 50) = **20.90 LSC**; × R = 0.0157168, rounded **down** to **0.0157 wTKA** |
| Reward ≤ penalty | 20.90 ≤ 1,590 (holds for any debt ≥ 500 LSC with these params) | self-liquidation nets −1,569.10 LSC |
| Lot to auction | collateral − reward | 12.50 − 0.0157 = **12.4843 wTKA** |
| Lot value | lot ÷ R | **16,601.46 LSC** |
| Expected raise | lot value × (1 − d), d ∈ [2%, 10%] | 16,269.43 … 14,941.32 LSC |
| Expected shortfall | target − raise | **1,221 … 2,549 LSC** |
| Gauge position | (CR − 100) ÷ 150 | 3.03% (104.5%), 34.65% (152.0%) |
| Other rows | a2…4e: 4.20 ÷ (4,410 × R) = 126.6%; 3f…b0: 6.00 ÷ (5,250 × R) = 152.0% | |
| Caps (counted on the auction target, Maker `hole`/`Hole` style) | wTKA: cap 60,000; in auction 21,560 (#40 8,910 + #41 9,350 + #42 3,300); +17,490 leaves **20,950**. Global: cap 250,000; in auction 61,560 (wTKA 21,560 + other types 40,000); leaves **170,950** | bars 35.93% + 29.15%, 24.62% + 7.00% |
| Allowed range | [R/2, 2R] | 0.000376 – 0.001504 |

## Screen 2 decisions
1. **Tokenised as `LogosDialog`, not the hard-coded dialogs.** It uses a backgroundSecondary `#262626` fill, a 1px `palette.border`, radiusLarge 8 and `palette.scrim` (LogosDialog.qml:11-12, :25-40). The title is 14 bold (the LogosDialog header), and the buttons sit right-aligned in `rightActions`. The behaviour comes from TransactionConfirmationDialog: modal, Cancel focused on open, no close while busy, and Esc cancels. The focus is drawn with LogosButton's real focus look (an overlayOrange border), not an outline.
2. **The trade is the hero.** "You pay 1,000 LSC → You get ≈ 0.802 wTKA" is set at 24 bold, with the source account and "minimum 0.798 · 0.5% slippage" underneath. Then come price used, discount at quote and network fee, followed by the note that the discount may be higher if the bid lands later but you never get less than 0.798.
3. **The mode is shown in the dialog itself** (a neutral PUBLIC / PRIVATE badge), so it doesn't depend on mockup labels.
4. **The privacy summary uses eye / eye-off icons, not green/yellow dots.** Green, yellow and red stay reserved for verdicts (v3 decision 3).
5. **The private steps are numbered** because they really are a sequence. Step 3 has a "Can't be skipped" badge, and the dialog ends with "Account A is never reused."
6. **The scrim covers only the 1100×720 plugin card.** The plugin is its own QQuickWidget, so a Popup overlay can't dim Basecamp's sidebar or tab strip. The two dialogs are shown side by side for review; in the app one dialog shows, centred.
7. **Post-submit uses `LogosStageLane`** in place of the bid card's button: "Submitted" (done) → "Including…" (busy, warning pulse) → "Done" (active green). When dropped, the Included stage goes to the failed red state, with "Not included after 5 blocks", "The bid was dropped, nothing was spent." and a primary Re-quote button. A private bid's lane is Deshield → Bidding… → Reshield, one stage per transaction.

### Bid arithmetic
You get ≈ 1,000 × 0.000752 ÷ (1 − 0.062) = 0.801706, shown as **0.802**. The minimum is 0.801706 × 0.995 = 0.79770, shown as **0.798** (as in v3; enforcing exactly 0.798 is 0.497% slippage). Quote time = 14:02 + 23m36s = **14:25:36**.

## New tokens (v4)
| CSS var | Value | Logos token | Defined |
| --- | --- | --- | --- |
| `--scrim` | `rgba(0,0,0,.5)` | `palette.scrim` = blackOpacity50 | DarkTheme.qml:77, ColorPalette.qml:15; used by LogosDialog.qml:27-31 |
| `--surface-recessed` | `#1F1F1F` | `palette.surfaceRecessed` = gray370 | DarkTheme.qml:17, ColorPalette.qml:28; LogosStageLane pending |
| `--error-border` | `rgba(251,55,72,.4)` | `palette.errorBorder` | DarkTheme.qml:51; LogosNotice Error |
| `--error-wash` | error @ 0.18 | LogosBadge `getColor(color, 0.18)` | LogosBadge.qml:14 |
| `--title` | 30px | `typography.titleText` | Typography.qml:33 (CR hero) |
| Stage-lane fills | done `#343434`, busy warning @ 0.14–0.28, active success @ 0.20, failed error @ 0.22 | LogosStageLane `d.fillFor` / `d.textFor` | LogosStageLane.qml:132-136 + fillFor/textFor |
| Secondary button | backgroundSecondary + `palette.border`; hover/focus backgroundMuted + overlayOrange border | LogosButton Secondary | LogosButton.qml:89-104, :133-139 |

Note: `v4-template.html` cited the scrim at DarkTheme.qml:79. It is at :77.

## Build map
| Element | QML |
| --- | --- |
| Page, header, tabs, list, panel | Unchanged from v3 (AppManagerView grid, LogosTabBar/LogosTabButton, LogosItemDelegate) |
| Position row | LogosItemDelegate contentItem: mono id + `LogosBadge` (error / neutral), CR in `subtitleText` bold, coloured by verdict |
| Scan status | LogosText 12 textTertiary + `LogosLink` "Rescan" (block scan from the last scanned height) |
| CR gauge | Item with 3 Rectangles (track split at 33.333%, 2px white threshold tick, 12×20 marker) + tick labels; marker x = clamp((CR − 100)/150) × width |
| Facts | GridLayout, 3 columns, as in v3 |
| Quote card | Inset Rectangle (background, borderSubtle, radiusLarge) + ColumnLayout of RowLayouts; caps = `LogosProgressBar` with a second fill Rectangle for "this liquidation" (accentOrange) after the used fill (textTertiary) |
| Disabled + reason | `LogosButton { enabled: false }` + LogosText 12 textSecondary centred below |
| Paused states | `LogosNotice` severity Warning (stale) / Error (out of range) |
| Bid dialog | `LogosDialog` with a custom contentItem; `rightActions: [LogosButton{Secondary "Cancel"}, LogosButton{Primary "Confirm bid"}]`; the primary label needs the black `contentItem` override noted in v3 |
| Reveal lists | Two inset Rectangles in a 2-column GridLayout, `LogosIcon` eye / eye-off tinted textSecondary |
| Private steps | Repeater of RowLayout: 20px circle (surface) + LogosText; `LogosBadge` neutral on step 3 |
| Post-submit | `LogosStageLane { stages: [Submitted, Included (busyLabel "Including…"), Done]; currentIndex; busy; failed }`, where failed = no signer-nonce advance within 5 blocks of submit. Private: Deshield / Bid / Reshield lane |
| Motion | LogosDialog scrim fade 120ms (built in); dialog enter opacity 0→1 + scale 0.96→1, 200ms ease-out, centred; button press scale 0.97 at 160ms as in v3 |

## Open decisions for the user
1. **Shortfall.** The brief's example (CR 104.5%, target = debt × 1.10) cannot reach the target at any discount. The screen says "Will end short" rather than hiding it. Either confirm that this is intended (it shows how bad debt is handled) or pick a hero position with CR ≥ 1.10 / 0.90 ≈ 122%.
2. **Keeper parameters.** tip = 5 LSC and cap = 50 LSC are my choice. The reward is valued at R and rounded down.
3. **Network fee is a placeholder:** ≈ 0.02 LGO per public tx, and 0.04 LGO for a private bid (bid + reshield, treating the deshield as fee-exempt). research-1 says v0.2.4 has no fees and v0.3 charges LGO, so the row should be hidden where there are no fees.
4. **Caps.** They count the auction target (Maker `tab`) against 60,000 (wTKA) and 250,000 (global). The values are illustrative.
5. **Dropped reshield.** The private lane only says "The reshield starts as soon as the bid is included." It is undecided what happens if the reshield itself is dropped: an automatic re-send, or a manual "Reshield now". The funds sit in A until it lands.
6. **"Nothing was spent."** This follows the brief (lez_core 0.4.1 silently drops failed txs). On v0.3 with fees, research-1 notes that public txs are charged even on failure, so the copy will need a variant there.
