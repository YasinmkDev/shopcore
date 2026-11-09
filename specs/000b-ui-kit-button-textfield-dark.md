# Spec 000b — ui-kit: Button, TextField + dark-theme wiring

Status: Draft (needs human approval before implementation)
Phase: 0   Depends on: 000 (scaffold)

## Goal
Finish the Phase 0 ui-kit skeleton: typed `Button` and `TextField` registry
components in white and dark themes, with the theme switch actually wired up.

## Users and scenario
A developer building the sell screen (spec 001) imports `Button` and
`TextField` from `@shopcore/ui-kit` instead of writing raw elements, and a
cashier on a dark shop floor can switch the app to the dark theme.

## In scope / Out of scope
In: `Button` (primary/secondary/ghost/danger; sm/md/lg; loading; icon-only
via `IconButton`), `TextField` (label, helper, inline error, disabled,
required), theme setting Light/Dark/Auto with persistence, Inter bundled
locally, a dev-only test page showing every variant/state in both themes,
migrating the scaffold Shell's raw Ping `<button>` to `Button`.
Out: `NumberField`/`MoneyField`/`QtyField` (build on `TextField`, later
specs), `Switch` (the theme control in the Shell is a plain select/buttons
demo, not the registry `Switch`), High contrast variant, density-mode
switching, `useMotionTier` hook, any core/Rust change, any DB change.

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
   `--focus-ring`) in both themes; focus is never removed via CSS.
6. Theme: `apps/ui` gains a theme provider/hook with setting Light
   (default) / Dark / Auto (follows OS via `matchMedia`
   `prefers-color-scheme`); it sets `data-theme` on the app root, so the
   dark block already in `tokens.css` takes effect. No component reads the
   OS setting on its own.
7. Theme choice persists per terminal in `localStorage` key
   `shopcore.theme` (proposed — see Q1; no DB/settings-store change).
8. The scaffold Shell migrates its raw Ping `<button>` to `Button`
   (primary, loading state while pinging) and gains a Light/Dark/Auto
   control built from the new components, proving the wiring end to end.
9. Inter variable font is bundled locally (see Dependencies): `font-family`
   becomes Inter first, system stack as fallback; no font is fetched from
   the network (offline-first). Tabular numerals stay on for money/qty.
10. Every component uses tokens only (Tailwind utilities mapped from
    `tokens.css`); no raw hex, no one-off styles. All user-visible strings
    in the Shell/test page come from i18n keys; component labels come from
    props (no hardcoded strings inside components).
11. Both components render correctly with `dir="rtl"` (logical properties
    only) and at 150% text scaling without breaking layout.
12. A dev-only test page in `apps/ui` (not in shipped navigation) shows
    every Button/TextField variant and state in both themes — the
    "Storybook-style test page" required by `10-ui-system.md`.

## Business rules touched (link to `context/02-domain-model.md` sections)
None. No money, pricing, tax, stock or sale rule is involved.

## Data changes (tables/columns/migrations/indexes; sync impact)
None. No migration. Theme preference lives in `localStorage` only.

## Core/API changes (commands, events, errors with codes, permissions)
None. No new Tauri command, no new IPC surface, `just gen-types` output
unchanged.

## UI (screens, components from registry, states, keyboard shortcuts, motion tier behavior)
Components: `Button`, `IconButton`, `TextField` in `packages/ui-kit/src/`,
exported from `packages/ui-kit/src/index.ts`. States per requirement 1/4,
plus loading (Button) and error (TextField). Keyboard: native button/input
behaviour — Enter/Space activates, Tab order sane, focus-visible ring.
Motion: requirement 2 only; the full `useMotionTier` hook and tier
measurement land with the first panel/drawer spec (see Q3).

## Performance (which budgets apply; expected numbers)
No hot-path code. Sell-screen JS budget (< 300 kB gzip) unaffected — the
font is a static asset, not JS; report the shipped woff2 size in the
session summary. No long tasks; press animation is transform-only.

## Security / audit (permissions, audit events)
None. Theme choice is a local display preference, not a sensitive action.

## Tests (list exactly what will be added: unit, property, integration, e2e, golden)
- Vitest component tests in `packages/ui-kit`: every Button variant/size/
  state (incl. loading + disabled not firing `onClick`), every TextField
  state (incl. error `aria-invalid`/`aria-describedby`, disabled,
  required), keyboard activation (Enter/Space), `IconButton` requires
  `aria-label`.
- Vitest tests in `apps/ui`: theme provider sets `data-theme="light"` /
  `"dark"` for Light/Dark, follows `matchMedia` for Auto, persists the
  choice to `localStorage` and restores it on load; Shell Ping still works
  through the new `Button` (existing Shell tests keep passing).
- RTL render test (`dir="rtl"`) for both components.
- AA contrast checks for text/accent/semantic colour pairs in both themes
  (computed from the token values, not eyeballed).
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

## Risks / open questions
- Q1: Theme persistence — `localStorage` per terminal for now, migrated to
  the core settings store when that spec lands? (Proposed: yes.)
- Q2: High contrast variant (`10-ui-system.md` mentions it) — in or out of
  000b? (Proposed: out, separate spec.)
- Q3: `useMotionTier` hook — build it now, or defer to the first
  panel/drawer spec and gate 000b motion on `prefers-reduced-motion` only?
  (Proposed: defer; 000b motion is one 80 ms transform.)
- Q4: Approve `@fontsource-variable/inter` 5.3.0 (verified on npm
  2026-11-09) as the local Inter source?
- Q5: Confirm sm Button (< 44 px) is back-office only.
- Q6: Density-mode switching (Comfortable/Compact) — deferred to the
  AppShell spec? (Proposed: yes; sm/md/lg are fixed sizes here.)

## Definition of done
Standard checklist in `AGENTS.md` + anything specific here: tests added
and passing, `just check` green, UI matches `10-ui-system.md` (all states,
keyboard-operable, i18n keys only, AA contrast, RTL checked), tracker +
decisions updated, session summary in the standard format.

## Dependencies proposed for approval
| Dependency | Version | Why |
|---|---|---|
| `@fontsource-variable/inter` | 5.3.0 | Bundles the Inter variable font locally (offline-first); `10-ui-system.md` requires Inter bundled locally, and `apps/ui/src/index.css` already notes it "lands with spec 000b" |
No other new dependency. No Radix primitive exists for Button/TextField
(ADR-010's Radix line covers overlay primitives for later components), no
new Rust crate. If 5.3.0 fails to resolve at install, use the nearest
stable version, report which one and why, and change nothing else.
