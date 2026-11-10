# Spec 000b — ui-kit: Button, TextField + dark-theme wiring

Status: Approved (with the changes below, 2026-11-10)
Phase: 0   Depends on: 000 (scaffold)

## Goal
Finish the Phase 0 ui-kit skeleton: typed `Button` and `TextField` registry
components in white and dark themes, with the theme switch actually wired up.

## Users and scenario
A developer building the sell screen (spec 001c) imports `Button` and
`TextField` from `@shopcore/ui-kit` instead of writing raw elements, and a
cashier on a dark shop floor can switch the app to the dark theme.

## In scope / Out of scope
In: `Button` (primary/secondary/ghost/danger; sm/md/lg; loading; icon-only
via `IconButton`), `TextField` (label, helper, inline error, disabled,
required), theme setting Light/Dark/Auto with persistence behind a
`SettingsStore` interface, `useMotionTier` hook (tiers 0–2), Inter bundled
locally (Latin subsets only), `--border-input` token, a dev-only test page
showing every variant/state in both themes, migrating the scaffold Shell's
raw Ping `<button>` to `Button`.
Out: `NumberField`/`MoneyField`/`QtyField` (build on `TextField`, later
specs), `Switch` (the theme control in the Shell is a plain select/buttons
demo, not the registry `Switch`), High contrast variant (out, separate
spec), density-mode switching (deferred to the AppShell spec), any core/Rust
change, any DB change.

## Functional requirements (numbered, testable)
1. `Button` renders primary/secondary/ghost/danger in sm/md/lg, with
   default, hover, focus-visible, active, disabled and loading states; the
   label is always text (never icon alone, except `IconButton`, which
   requires `aria-label` + tooltip).
2. `Button` press is transform-only (`scale .98`, 80 ms, token
   `--dur-instant`); it never animates width/height/shadow. Under
   `prefers-reduced-motion` the press animation is off.
3. `Button` md/lg meet the 44 px minimum target in POS mode; sm is
   back-office only and is labelled as such in the test page.
4. `TextField` renders label, helper text, placeholder, required marker,
   disabled and error states; error text sits inline next to the field in
   plain language, with `aria-invalid` + `aria-describedby`.
5. `TextField` has a visible 2 px focus ring + offset (token
   `--focus-ring`) in both themes; focus is never removed via CSS. The
   focus ring colour is ≥ 3:1 against the surface in both themes (tested,
   see Tests).
6. Theme: `apps/ui` gains a theme provider/hook with setting Light
   (default) / Dark / Auto (follows OS via `matchMedia`
   `prefers-color-scheme`); it sets `data-theme` on the app root, so the
   dark block already in `tokens.css` takes effect. No component reads the
   OS setting on its own.
7. Theme choice persists per terminal behind a small `SettingsStore`
   interface (read/write `theme`); the 000b implementation is backed by
   `localStorage` key `shopcore.theme`. Moving to the core settings store
   later is a one-file change (swap the backing store, not the call sites).
8. `data-theme` is set synchronously before the first React render (no
   flash of the wrong theme): a tiny inline script in `index.html` (or the
   equivalent entry point) reads the stored setting and sets the attribute
   before React mounts.
9. The scaffold Shell migrates its raw Ping `<button>` to `Button`
   (primary, loading state while pinging) and gains a Light/Dark/Auto
   control built from the new components, proving the wiring end to end.
10. Inter variable font is bundled locally (see Dependencies): `font-family`
    becomes Inter first, system stack as fallback; no font is fetched from
    the network (offline-first). Latin subsets only. The OFL license text
    ships with the bundle. Tabular numerals stay on for money/qty.
11. Every component uses tokens only (Tailwind utilities mapped from
    `tokens.css`); no raw hex, no one-off styles. All user-visible strings
    in the Shell/test page come from i18n keys (`t()` from
    `@shopcore/i18n` — confirmed to exist, see Dependencies); component
    labels come from props (no hardcoded strings inside components).
12. Both components render correctly with `dir="rtl"` (logical properties
    only) and at 150% text scaling without breaking layout.
13. A dev-only test page in `apps/ui` (not in shipped navigation) shows
    every Button/TextField variant and state in both themes — the
    "Storybook-style test page" required by `10-ui-system.md`.
14. `--border-input` token: a border colour ≥ 3:1 against the surface in
    both themes, used for text-input borders (replacing the decorative
    `--border` on inputs). The exact value is chosen by the implementer;
    the test proves the ratio.
15. `useMotionTier` hook (per `10-ui-system.md` adaptive tiers): tiers
    0 (full, p95 frame ≤ 20 ms), 1 (reduced, p95 > 20 ms — no
    layout/shared-element animations or springs; fades/slides kept),
    2 (minimal, p95 > 33 ms — opacity-only, ≤ 100 ms). OS
    `prefers-reduced-motion` forces tier 2. The hook sets a `data-motion`
    attribute (`full` / `reduced` / `minimal`) on the app root. A basic
    rAF frame-time sampler measures p95 over the first 60 frames after
    startup and during the first transitions. User override
    (Full / Reduced / Off) lives in Settings → Appearance → Motion and is
    stored per terminal behind the same `SettingsStore` interface. No
    component decides the tier on its own. Spec 001c's payment panel
    needs this hook.

## Business rules touched (link to `context/02-domain-model.md` sections)
None. No money, pricing, tax, stock or sale rule is involved.

## Data changes (tables/columns/migrations/indexes; sync impact)
None. No migration. Theme and motion preferences live behind
`SettingsStore` (localStorage-backed in 000b) only.

## Core/API changes (commands, events, errors with codes, permissions)
None. No new Tauri command, no new IPC surface, `just gen-types` output
unchanged.

## UI (screens, components from registry, states, keyboard shortcuts, motion tier behavior)
Components: `Button`, `IconButton`, `TextField` in `packages/ui-kit/src/`,
exported from `packages/ui-kit/src/index.ts`. States per requirement 1/4,
plus loading (Button) and error (TextField). Keyboard: native button/input
behaviour — Enter/Space activates, Tab order sane, focus-visible ring.
Motion: requirement 2 (Button press) plus the `useMotionTier` hook
(requirement 15); the hook is in `packages/ui-kit` and consumed by
`apps/ui`. The sell-screen motion exceptions in `10-ui-system.md` are
gated on the tier (tier 2 = opacity-only).

## Performance (which budgets apply; expected numbers)
No hot-path code. Sell-screen JS budget (< 300 kB gzip) unaffected — the
font is a static asset, not JS; report the shipped woff2 sizes (per
subset) in the session summary. The rAF sampler is cheap (60 frames,
then idle); no long tasks; press animation is transform-only.

## Security / audit (permissions, audit events)
None. Theme and motion choices are local display preferences, not
sensitive actions.

## Tests (list exactly what will be added: unit, property, integration, e2e, golden)
- Vitest component tests in `packages/ui-kit`: every Button variant/size/
  state (incl. loading + disabled not firing `onClick`), every TextField
  state (incl. error `aria-invalid`/`aria-describedby`, disabled,
  required), keyboard activation (Enter/Space), `IconButton` requires
  `aria-label`.
- Vitest tests in `apps/ui`: theme provider sets `data-theme="light"` /
  `"dark"` for Light/Dark, follows `matchMedia` for Auto, persists the
  choice via `SettingsStore` and restores it on load; `data-theme` is set
  before first render (no-flash test); Shell Ping still works through the
  new `Button` (existing Shell tests keep passing).
- `useMotionTier` unit tests: tier 0 at p95 ≤ 20 ms, tier 1 at p95 > 20 ms,
  tier 2 at p95 > 33 ms, `prefers-reduced-motion` forces tier 2, user
  override (Full/Reduced/Off) wins over the sampler, `data-motion`
  attribute set on the root.
- RTL render test (`dir="rtl"`) for both components.
- AA contrast checks computed from the token values (not eyeballed),
  covering EVERY text/background pair in both themes: text, text-muted,
  text-faint (placeholders only), text-on-accent, accent and accent-subtle
  (text on accent-subtle and accent-subtle as a background), and semantic
  text on its tint background (success/warning/danger/info on
  success-bg/warning-bg/danger-bg/info-bg). `--border-input` ≥ 3:1
  against the surface in both themes; focus ring ≥ 3:1 against the
  surface in both themes.
- `just check` green (fmt, clippy, deny, nextest, tsc, eslint, vitest).
- Manual click-through on Windows (still open from spec 000): Ping via the
  new `Button`, plus theme toggle Light → Dark → Auto.

## Acceptance criteria (Given/When/Then)
- Given the light theme, when I render every Button variant and TextField
  state on the test page, then all match the tokens with no raw values.
- Given the theme set to Dark, when the app loads, then `data-theme="dark"`
  is set, surfaces/text use the dark token values, and StatusPill, Button
  and TextField stay AA-contrast legible.
- Given the theme set to Auto and the OS in dark mode, when the app loads,
  then it renders dark; when I pick Light, then it renders light and stays
  light on reload.
- Given a disabled Button, when I click or press Enter/Space on it, then
  nothing happens.
- Given a TextField with an error, when it renders, then the error text is
  inline next to the field and the input carries `aria-invalid`.
- Given the app offline, when it loads, then Inter renders from the local
  bundle with no network font request.
- Given `prefers-reduced-motion` on, when the app loads, then
  `data-motion="minimal"` is set and animations are opacity-only.
- Given a TextField, when it renders, then its border is the
  `--border-input` token at ≥ 3:1 against the surface.

## Risks / open questions
- Q1: RESOLVED (approved 2026-11-10) — `SettingsStore` interface backed by
  `localStorage` in 000b; core settings store later is a one-file change.
- Q2: RESOLVED (approved 2026-11-10) — high contrast out, separate spec.
- Q3: RESOLVED (approved 2026-11-10, reversed) — `useMotionTier` IS in
  000b (requirement 15); spec 001c's payment panel needs it.
- Q4: RESOLVED (approved 2026-11-10) — `@fontsource-variable/inter`
  5.3.0, Latin subsets only, OFL text ships, woff2 sizes reported. If
  5.3.0 does not resolve, STOP and ask; do not substitute a version.
- Q5: RESOLVED (approved 2026-11-10) — sm Button is back-office only.
- Q6: RESOLVED (approved 2026-11-10) — density-mode switching deferred to
  the AppShell spec.

## Definition of done
Standard checklist in `AGENTS.md` + anything specific here: tests added
and passing, `just check` green, UI matches `10-ui-system.md` (all states,
keyboard-operable, i18n keys only, AA contrast, RTL checked), tracker +
decisions updated, session summary in the standard format.

## Dependencies proposed for approval
| Dependency | Version | Why |
|---|---|---|
| `@fontsource-variable/inter` | 5.3.0 | Bundles the Inter variable font locally (offline-first); `10-ui-system.md` requires Inter bundled locally, and `apps/ui/src/index.css` already notes it "lands with spec 000b". Latin subsets only; OFL text ships. |
No other new dependency. `t()` and the `@shopcore/i18n` package are
CONFIRMED to exist (`packages/i18n/src/index.ts` exports `t(key, locale)`,
already imported by `apps/ui/src/features/shell/Shell.tsx`) — no scaffold
needed. No Radix primitive exists for Button/TextField (ADR-010's Radix
line covers overlay primitives for later components), no new Rust crate.
If 5.3.0 fails to resolve at install, STOP and ask; do not substitute a
version.
