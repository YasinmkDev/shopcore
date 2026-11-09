# 10 — UI/UX System (mandatory for any UI work)

## Design intent
Calm, precise, fast. It should feel like a very good ledger or a well-made instrument, not a startup landing page. White-first. One accent color. Dense where work is dense, generous where reading matters. Motion is fluid and purposeful, and never slows the cashier down.

## Principles
1. **Speed over spectacle at checkout.** The sell screen is nearly motionless; every millisecond belongs to the cashier.
2. **Keyboard-first, touch-ready.** Every action reachable by keyboard; every control ≥ 44 px in POS mode.
3. **One obvious next action.** Primary action visible, labeled, and bound to a key.
4. **Never block, never lose.** No modal chains in a sale. Errors say what to do next. Destructive actions are undoable or approval-gated.
5. **Always show state.** Online/offline, sync, printer, shift, and user are visible at all times in the status bar.
6. **Consistency through tokens and registry.** No one-off styles.

## Banned
Gradients, glow/neon, glassmorphism/blur backdrops, purple-blue "AI" palettes, emoji as icons, decorative blobs/illustrations, oversized pill cards, drop-shadow stacks, animated count-up numbers, parallax, auto-playing motion, dark patterns.

## Theme tokens (white default)
Implement as CSS variables in `packages/ui-kit/tokens.css`; Tailwind maps to them. Dark theme overrides the same variables via `[data-theme="dark"]`. Components never use raw hex.

```css
:root {
  /* surfaces */
  --bg:            #FFFFFF;  /* app background */
  --surface:       #FFFFFF;  /* cards, panels */
  --surface-subtle:#F7F7F5;  /* table headers, sidebars, hover rows */
  --surface-sunken:#F0EFEC;  /* inputs on subtle, wells */
  --border:        #E5E4E0;
  --border-strong: #CFCEC8;

  /* text */
  --text:          #1B1B1A;
  --text-muted:    #6B6A66;
  --text-faint:    #8F8E89;  /* placeholders only; never for essential text */
  --text-on-accent:#FFFFFF;

  /* accent (single) */
  --accent:        #0F766E;
  --accent-hover:  #0D6962;
  --accent-active: #0B5A54;
  --accent-subtle: #E6F4F2;  /* selected rows, soft buttons */
  --focus-ring:    #0F766E;

  /* semantic */
  --success:#15803D; --success-bg:#ECFDF3;
  --warning:#B45309; --warning-bg:#FFF7E6;
  --danger: #B91C1C; --danger-bg:#FEF1F1;
  --info:   #1D4ED8; --info-bg:#EEF3FF;

  /* shape + elevation */
  --radius-sm:4px; --radius:6px; --radius-lg:8px;
  --shadow-pop: 0 4px 16px rgba(27,27,26,.10), 0 1px 3px rgba(27,27,26,.06); /* popovers/dialogs only */

  /* spacing grid = 4px; use multiples */
}
[data-theme="dark"] {
  --bg:#121211; --surface:#1B1B1A; --surface-subtle:#222120; --surface-sunken:#181817;
  --border:#2C2B29; --border-strong:#3A3936;
  --text:#ECEBE8; --text-muted:#A3A19B; --text-faint:#7C7A75; --text-on-accent:#06201D;
  --accent:#14B8A6; --accent-hover:#2DD4BF; --accent-active:#5EEAD4; --accent-subtle:#10312E; --focus-ring:#2DD4BF;
  --success:#4ADE80; --success-bg:#0F2A1A; --warning:#FBBF24; --warning-bg:#2E2208;
  --danger:#F87171; --danger-bg:#331414; --info:#7DA2FF; --info-bg:#14203F;
  --shadow-pop: 0 4px 16px rgba(0,0,0,.5);
}
```
Rules: contrast AA minimum for all text (verified in tests); never rely on color alone (pair with icon/text); status colors only for status; accent only for primary actions, selection, focus, and links. Theme setting: Light (default), Dark, Auto (follows OS). A "High contrast" variant increases border and text contrast.

## Typography
- Font: **Inter** (bundled locally, variable); fallbacks `system-ui, "Segoe UI", sans-serif`; RTL/other scripts: Noto Sans Arabic / Noto Nastaliq Urdu / others bundled as needed.
- **Tabular numerals** (`font-variant-numeric: tabular-nums`) for all money and quantities.
- Scale (px): 12 caption, 13 dense-table, 14 body (back office), 16 body (POS), 18 section, 22 page title, 28 emphasis, 40 POS grand total (bold). Line-height 1.4 body, 1.2 headings. Weights 400/500/600 only.
- Money alignment: right-aligned, fixed decimals, currency symbol de-emphasized.

## Shape, spacing, elevation, icons
- 4 px grid. Radius 6 px (4 px inputs-dense, 8 px dialogs). 1 px borders instead of shadows; shadow only on popovers, menus, dialogs, drawers.
- Icons: Lucide, 1.5 px stroke, 16/20 px, always with text on primary actions; icon-only buttons need tooltip + aria-label.
- Density modes: **Comfortable** (POS/touch: 48 px rows/targets) and **Compact** (back office: 32 px rows). Per-surface default, user-switchable.

## Sell screen layout (core of the product)
```
┌──────────────────────────────────────────────────────────────────────────────┐
│ [Scan / search item or customer............................] [F2]   Cashier ▾ │
├──────────────────────────────────────────────┬───────────────────────────────┤
│ CART                                          │  Customer: Walk-in     [F4]  │
│ #  Item              Qty   Price    Total     │                               │
│ 1  Rice 5kg           2   12.00    24.00  ✕   │  Subtotal            48.00   │
│ 2  Milk 1L            1    3.50     3.50  ✕   │  Discount             0.00   │
│ ...                                           │  Tax                  4.32   │
│ (virtualized, selected row highlighted)       │  ─────────────────────────── │
│                                               │  TOTAL            52.32 (40px)│
│ Quick items: [Bag] [Bread] [Water] ...        │  [Hold F8] [Discount F6]     │
│                                               │  [ PAY  F9 ]  (accent, big)  │
├──────────────────────────────────────────────┴───────────────────────────────┤
│ ● Online  ⟳ Synced 12:04  🖨 Ready  Shift #12  Till 3  Cashier: Ana  F1 Help │
└──────────────────────────────────────────────────────────────────────────────┘
```
(Status bar uses Lucide icons, not emoji.) Left ~2/3 cart, right ~1/3 totals and actions. Scan input always focused. Payment opens as a **side panel**, not a modal: tender buttons (Cash/Card/Wallet/Credit), numpad, quick cash amounts (exact, next round numbers), live change due, split payment list. Complete with Enter. After completion: brief confirmation, receipt prints, cart clears, focus returns to scan input within 100 ms.

## Back-office layout
Left nav rail (collapsible, icons + labels), top bar with branch switcher, search/command palette (Ctrl+K), sync/offline pill, user menu. Content: page title + primary action top-right, filter/saved-view bar, dense virtualized table, details in a right-side drawer (not a new page) for quick edits. Bulk edit grid for catalog/stock (spreadsheet-like: arrow keys, Enter to commit, paste from Excel, undo).

## Keyboard map (defaults, rebindable)
F1 help/shortcuts · F2 search · F3 price check · F4 customer · F5 refresh/sync status · F6 discount · F7 returns · F8 hold/recall · F9 pay · F10 manager menu · Ctrl+K command palette · Ctrl+Z undo last line action · Esc cancel/close · Enter confirm · ↑/↓ cart selection · +/− qty · Del remove line · * set quantity then scan. Shortcut hints visible in buttons and a bottom strip.

## Component registry (use these; add new ones here first)
AppShell, NavRail, TopBar, StatusBar, StatusPill (online/sync/printer/shift), CommandPalette, ScanInput, CartTable, CartRow, TotalsPanel, PaymentPanel, NumPad, TenderButton, ManagerPinPopover, QuickItemTile, DataTable (virtualized, sortable, column control, saved views), BulkEditGrid, FilterBar, Drawer, Dialog, Popover, Menu, Tooltip, Toast, Banner, EmptyState, Skeleton, Tabs, Stepper (wizard), Badge, Button (primary/secondary/ghost/danger; sm/md/lg), IconButton, TextField, NumberField, MoneyField, QtyField, Select, Combobox (async, virtualized), DatePicker, Switch, Checkbox, RadioGroup, ShortcutHint, KeyCap, ReceiptPreview, LineChart (uPlot), BarChart (SVG).
For each: props typed, all states (default, hover, focus-visible, active, disabled, loading, error), keyboard behavior, RTL, density variants, Storybook-style test page. Built on Radix primitives where available.

## Required states for every screen
Loading (skeleton, no spinner walls), empty (explain + one action), error (what happened + what to do + retry), offline (banner + what still works), no-permission (explain + request approval), success (brief, non-blocking toast). Form errors inline next to fields, in plain language, focus moved to first error.

## Motion system (fluid, purposeful, cheap)
Tokens:
```css
--dur-instant:80ms; --dur-fast:120ms; --dur-base:180ms; --dur-slow:260ms;
--ease-out:   cubic-bezier(0.16, 1, 0.3, 1);   /* entrances */
--ease-in:    cubic-bezier(0.4, 0, 1, 1);      /* exits */
--ease-inout: cubic-bezier(0.4, 0, 0.2, 1);    /* moves */
/* spring (motion lib) for panels/drawers: stiffness 420, damping 36, mass 1 */
```
Rules:
- Animate **only `transform` and `opacity`**. Never animate width/height/top/left, large blurs, or box-shadow. Use `will-change` sparingly and remove after.
- Entrances use ease-out, exits use ease-in and are faster (~60% of entrance).
- Max concurrent animated elements ~6. No animation over 300 ms except page-level transitions (≤ 260 ms).
- Always interruptible; never block input while animating.

Choreography:
| Element | Motion |
|---|---|
| Drawer / side panel | slide from edge (transform) + fade, spring |
| Dialog | opacity 0→1 + scale .98→1, 180 ms |
| Popover/menu | opacity + 4 px translate, 120 ms |
| Route change (back office) | View Transition crossfade, 150 ms |
| Tab indicator, selection highlight | shared-layout move (motion), spring |
| Toast | slide-in 8 px + fade, auto-dismiss, pause on hover |
| List reorder / row insert (back office) | layout animation (motion) |
| Button press | scale .98, 80 ms |
| Skeleton | slow opacity pulse (no shimmer gradients) |
| Focus ring | instant (no transition) |

**Sell screen exceptions (strict):** cart row insert = 100 ms opacity + 4 px translate; total changes = 250 ms background tint fade on the total (no count-up); payment panel = 150 ms slide. Nothing else animates in checkout.

**Adaptive tiers.** Measure frame times (rAF sampling over first 60 frames after startup and during the first transitions).
- Tier 0 (full): p95 frame ≤ 20 ms.
- Tier 1 (reduced): p95 > 20 ms → disable layout/shared-element animations and springs; keep fades/slides.
- Tier 2 (minimal): p95 > 33 ms, or OS `prefers-reduced-motion` → opacity-only, ≤ 100 ms.
User can override in Settings → Appearance → Motion (Full / Reduced / Off). Tier stored per terminal. Animation code reads the tier from one hook (`useMotionTier`); no component decides on its own.

## Accessibility
AA contrast, visible focus on every interactive element (2 px ring + offset), full keyboard operation, logical focus order, focus trapping in dialogs with restore on close, ARIA roles via Radix, live regions for totals/errors/scan feedback, minimum 44 px targets in POS mode, text scalable to 150% without breaking layout, no information by color alone, screen-reader labels on icon buttons.

## Internationalization
All strings via keys; ICU plural/number/date formatting; currency formatting per tenant; full RTL mirroring using logical properties; numeral system option (Latin/Arabic-Indic); long-string tolerance (+40% width). Receipts support the same languages (see `06-hardware-integrations`).

## UX writing
Plain, short, active voice. Buttons are verbs ("Complete sale", "Add item"). Errors say what to do. No jargon, no exclamation marks, no blame.

## Onboarding and first run
Setup wizard: business → branch/till → tax → currency/language → receipt → printer/scanner test → import data → create users/PINs. **Training mode** (separate DB, banner always visible) for new cashiers. Contextual empty states guide first actions.

## UI acceptance checklist (per screen)
Matches tokens (no raw values) · uses registry components · all required states · keyboard-only operable · tab order sane · RTL checked · density modes checked · motion follows tiers · within perf budgets (`05`) · i18n keys only · AA contrast · works offline.
