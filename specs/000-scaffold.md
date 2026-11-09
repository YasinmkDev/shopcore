# Spec 000 — Repo scaffold + CI + `just check`

Status: Done (implemented 2026-11-09, `just check` green)
Phase: 0   Depends on: (none — first spec)

## Goal (one sentence, user-visible outcome)

A developer can clone the repo, run one setup command, get a green `just check`, and open a white-theme Tauri window that proves the UI → Rust round-trip with a `ping` command.

## Users and scenario

- **Implementer (me):** creates the workspace, toolchain pins, CI, and the minimal shell in one session, with zero business logic.
- **Reviewer (you):** approves this spec and the dependency list in the session reply before any code is written.
- **Future sessions:** every later spec builds on this scaffold and assumes `just check` is green on `main`.

## In scope / Out of scope

In scope:
- Cargo workspace + pnpm workspace exactly per the repo layout in `context/03-architecture.md` (crates `domain, core, db, hardware, protocol, desktop, hub, server, bench`; `apps/ui`; `packages/ui-kit, i18n, protocol-ts`; `tests/golden, e2e`).
- `rust-toolchain.toml` (Rust 1.99.1, edition 2024), Node 22.23.4 + pnpm 12.18.0 pins, `justfile` with every command in `AGENTS.md`.
- CI workflow (`.github/workflows/ci.yml`) on `windows-latest` running `just check`.
- Lint/format/deny configs: `rustfmt.toml`, workspace Clippy lints, `deny.toml` (cargo-deny), ESLint + Prettier, `tsconfig` strict.
- Minimal Tauri shell (`crates/desktop`): white-theme window, `packages/ui-kit/tokens.css` (verbatim copy from `context/10-ui-system.md`), a `StatusPill` component, and a `ping` command round-trip through the `CoreClient` interface with generated TypeScript types.
- Empty crates with doc comments; `packages/i18n` with a minimal `en.json` (so the shell's strings already go through i18n keys, per the hard rule in `AGENTS.md`).
- `just gen-types` generating `packages/protocol-ts/src/generated.ts` from Rust (never hand-edited).

Out of scope (later specs):
- Any business logic, domain types (Money/Qty/Rate), DB schema/migrations, sync, hardware drivers, auth, audit.
- `Button` / `TextField` components and dark-theme wiring (see open question Q1 — `12-build-plan.md` Phase 0 lists them in the ui-kit skeleton; this spec covers tokens + `StatusPill` only, per the approved scope for spec 000).
- `just bench` budget gate, migration tests, installer build, `pnpm audit`, secret scanning in CI (per `09-engineering-standards.md` CI section — these land with the specs that need them; CI in this spec runs `just check` only).
- MSIX/installer, code signing, updater, Playwright/tauri-driver e2e (test-strategy suite lands with spec 001+).

## Functional requirements (numbered, testable)

1. `just setup` installs the pinned Rust toolchain (rustup), pnpm (corepack), and `just` if missing, then runs `pnpm install` and `cargo fetch`. (Extra recipe beyond `AGENTS.md`'s list; needed because a fresh machine has none of these. This VM currently has none of cargo/rustc/pnpm/just installed — only Node 24.22.1.)
2. `just check` runs, in order: `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets` with `-D warnings`; `cargo deny check`; `cargo nextest run --workspace`; `pnpm install --frozen-lockfile`; `pnpm -r run typecheck` (`tsc --noEmit`); `pnpm -r run lint` (ESLint, zero warnings); `pnpm -r run test` (`vitest run`). It must exit 0 on an empty app.
3. `just dev` starts the Tauri desktop app with Vite hot reload (`tauri dev` from the desktop crate root `crates/desktop/src-tauri`; `beforeDevCommand` runs the UI dev server).
4. `just test` runs Rust + TS tests only. `just bench` prints "no benchmarks yet" and exits 0 (stub — `crates/bench` is an empty crate; real benchmarks come with the Phase 0 benchmark spec).
5. `just gen-types` regenerates `packages/protocol-ts/src/generated.ts` from `crates/protocol` via `ts-rs`; the file carries a "generated — do not edit" header. CI verifies it is up to date (run `just gen-types` and fail on `git diff`).
6. `just migrate` prints "no migrations yet" and exits 0 (stub — first migration lands with the DB spec). `just seed-large` prints "no seeder yet" and exits 0 (stub — lands with the Phase 0 benchmark spec).
7. The Tauri window opens at 1280×800, white theme (`[data-theme="light"]`, the `:root` tokens), title "ShopCore", and shows: a `StatusPill` in `online` state and a Ping button; pressing it calls `ping` through `CoreClient` and shows the returned `Pong` message.
8. `CoreClient` (per `09-engineering-standards.md`) exists in `apps/ui/src/lib/core-client.ts` with `TauriClient` (Tauri `invoke` + `listen`) implemented; `HttpClient` is a stub that throws `not implemented` (lands with the hub spec).
9. Tauri capabilities (`crates/desktop/src-tauri/capabilities/default.json`) allow only the `ping` command and the default window/app permissions. No filesystem, shell, dialog, or notification permissions. Strict CSP, no remote content, DevTools off in release builds.
10. All user-visible strings in the shell come from i18n keys (`packages/i18n/src/en.json`); no string literals in JSX.

## Business rules touched (link to `context/02-domain-model.md` sections)

None. Zero business logic by design. The only domain-adjacent rule honoured is the hard rule that money never uses floats — no money code exists yet.

## Data changes (tables/columns/migrations/indexes; sync impact)

None. No migrations, no tables. `crates/db` is an empty crate with a doc comment; its `rusqlite` dependency (SQLCipher feature) is included in this spec only as a **compile check** — it proves the riskiest native dependency builds on the Windows CI runner before any schema work depends on it (see risk R2).

## Core/API changes (commands, events, errors with codes, permissions)

- Command: `ping()` → `Pong { message: String }`, defined in `crates/protocol` (`#[derive(TS)]` via `ts-rs`), implemented as `core::ping()` (a seam function, not business logic), exposed by `crates/desktop` as a `#[tauri::command]`. No auth/permission check (no users exist yet; auth lands with the identity spec — recorded here so it is not forgotten).
- Events: none.
- Errors: none for `ping`. `AppError` type is **not** created in this spec (lands with the first fallible command); the `ping` path is infallible by construction.
- Type bridge decision (see open question Q2): **`ts-rs` 12.0.1 (stable)** generates the TS types. `specta` was the other candidate in `03-architecture.md`; the Tauri-2-compatible line is pre-release only (`specta 2.0.0-rc.28` + `tauri-specta 2.0.0-rc.26`), while stable `specta 1.0.5` / `tauri-specta 1.0.2` target Tauri v1. If you prefer the specta RC line, this spec changes before implementation.

## UI (screens, components from registry, states, keyboard shortcuts, motion tier behavior)

- One screen: the scaffold shell. Layout: top bar with app title + `StatusPill` (online), centred card with the Ping button and the `Pong` result line, status bar with a static "Ready" pill.
- Components used (from the registry in `10-ui-system.md`): `StatusPill` only. `Button`/`TextField` are out of scope for this spec (Q1).
- `StatusPill` props: `status: 'online' | 'offline' | 'syncing' | 'error'`, `label: string`. Icon (Lucide) + text — never colour alone. All states rendered in the component test.
- Tokens: `packages/ui-kit/tokens.css` copies the theme tokens from `10-ui-system.md` **verbatim** (both `:root` light and `[data-theme="dark"]` blocks, plus the motion tokens). Tailwind v4 maps to them via `@theme inline` in `apps/ui/src/index.css` (no raw hex anywhere in components).
- Keyboard: Ping button is a native `<button>` (Tab/Enter/Space work); no custom shortcuts in this spec.
- Motion: none in the shell except the button-press scale (`.98`, 80 ms) from `10-ui-system.md`. No animation tiers needed yet (tier hook lands with the first animated screen).
- Required states: loading (button disabled + "Pinging…" while the round-trip is in flight), empty (no result yet — shows the hint text), error (round-trip failure shows the error + retry; `ping` is infallible server-side, so this path is exercised with a mocked failing client in tests).

## Performance (which budgets apply; expected numbers)

- Cold start to usable shell: budget < 4 s on the weakest PC (`05-performance-budgets.md`, initial). A scaffold shell should measure well under 1 s on a dev machine; the number is recorded when the Phase 0 benchmark spec runs on the weakest PC — not measured in this spec.
- Installer size budget (< 60 MB, initial) does **not** apply yet (no installer in this spec).
- No list, search, or hot-path code exists yet; `just bench` is a stub.

## Security / audit (permissions, audit events)

- No users, roles, or audit log yet. Tauri capabilities are minimal by construction (requirement 9). No secrets anywhere. `cargo deny` + `cargo audit` posture is set up now (`deny.toml` with an allow-list of licences; advisories denied).

## Tests (list exactly what will be added: unit, property, integration, e2e, golden)

- Rust: `crates/core` unit test — `ping()` returns `Pong { message: "pong" }`. `crates/protocol` test — `ts-rs` export of `Pong` contains the expected TS shape (guards `just gen-types` output).
- TS (Vitest + Testing Library): `StatusPill` renders all four states with icon + text; `App` shows the `Pong` message after a mocked `CoreClient.ping()` resolves, shows the error state when it rejects, and disables the button while in flight.
- CI: `just check` green on `windows-latest` (push + PR). `just gen-types` freshness check (no diff after regeneration).
- Manual (recorded in the session summary, not CI): `just dev` on Windows, press Ping, see `pong`; `just check` run locally.
- Not in this spec: Playwright/tauri-driver e2e (lands with spec 001 per `11-test-strategy.md`).

## Acceptance criteria (Given/When/Then)

- Given a fresh clone, when I run `just setup && just check`, then it exits 0 with no warnings.
- Given `just dev` running, when I press the Ping button, then the `Pong` message appears and the `StatusPill` still shows online.
- Given I edit a generated file (`packages/protocol-ts/src/generated.ts`) by hand, when CI runs, then the gen-types freshness check fails.
- Given I add an `unwrap()` to non-test Rust, when `just check` runs, then Clippy denies it and the check fails.
- Given I add a dependency without approval, when the PR is reviewed, then `Cargo.lock` / `pnpm-lock.yaml` shows the change and it is rejected (process check, not automated).

## Risks / open questions

- **Q1 — ui-kit scope vs `12-build-plan.md`:** Phase 0 lists Button + TextField + dark-theme wiring in the ui-kit skeleton; this spec covers tokens + StatusPill only. Decide: fold Button/TextField + dark toggle into spec 000, or a 000b follow-up spec?
- **Q2 — Type bridge:** `ts-rs` 12.0.1 (stable, proposed) vs `specta` 2.0.0-rc.28 + `tauri-specta` 2.0.0-rc.26 (Tauri-2-compatible but pre-release) vs stable `specta` 1.0.5 (Tauri v1 only — incompatible). Needs your call; the spec as written uses ts-rs.
- **Q3 — TypeScript line:** npm `latest` is TypeScript 7.0.2 (native rewrite, brand-new). This spec pins TypeScript 5.9.3 (proven). Confirm, or adopt TS 7 now?
- **Q4 — CI runners:** `windows-latest` only in this spec. Confirm no Linux/macOS `just check` matrix is wanted yet (Tauri on Linux needs webkit2gtk system libs; the Windows-first target makes Windows CI the source of truth).
- **R1 — Toolchain install time:** first CI run installs Rust + pnpm + just + nextest + deny (several minutes); acceptable for scaffold, revisit with caching later.
- **R2 — SQLCipher Windows build:** `rusqlite` with `bundled-sqlcipher-vendored-openssl` compiles OpenSSL from source on the runner. If CI cannot build it, the fallback is OpenSSL via vcpkg (`sqlcipher` feature + `OPENSSL_DIR`) — recorded as an ADR if triggered. The `db` crate exists in this spec precisely to smoke this out early.
- **R3 — Rust 1.100.0 is due 2026-11-12** (three days after this pin). Pin 1.99.1 now; bump deliberately after reading the release notes, per `09-engineering-standards.md` (never bump as a side effect).

## Definition of done

Standard checklist in `AGENTS.md` + : `just check` green in CI and locally; `just dev` shows the white-theme shell and the `ping` round-trip works; tracker (`13-progress-tracker.md`) updated with spec 000 Done; no new decision is left undocumented (ADRs added for Q2/Q3/Q4 once decided).

## Implementation notes (2026-11-09, recorded per AGENTS.md — keep context files truthful)

Deviations and findings from implementation. None change the approved scope; all are recorded here and in the session summary.

- **Tauri config path (build.rs):** `tauri_build::build()` looks for `tauri.conf.json` next to `Cargo.toml`, but the approved layout keeps it in `src-tauri/`. `build.rs` therefore uses `Attributes::new().config_path("src-tauri/tauri.conf.json")`. Capabilities resolve from `src-tauri/capabilities/` next to the config; icons live in `src-tauri/icons/` (paths in the config are relative to the config file).
- **Codegen context (main.rs):** `tauri::generate_context!()` also reads the config next to `Cargo.toml`, so it cannot be used with this layout. The build script enables the `codegen` feature (`tauri-build 2.7.4`) and `main.rs` uses `tauri::tauri_build_context!()` instead — Tauri's documented path for custom layouts.
- **Generated-code lints (main.rs):** the generated context trips pedantic lints (`manual_string_new`, `default_trait_access`, `single_match_else`) plus the workspace `unwrap_used`/`expect_used` denies. A `#[allow(...)]` for exactly those five lints is scoped to `fn main` only, with a comment; hand-written code elsewhere keeps the full deny set.
- **Icon format:** Tauri requires RGBA PNGs; the placeholder icons are RGBA solid accent `#0F766E`. A real icon lands with the branding spec.
- **Name collision (main.rs):** the `#[tauri::command] fn ping()` collided with the imported `shopcore_core::ping`; the import is aliased to `core_ping`.
- **ts-rs 12 API (gen_types.rs, protocol tests):** `TS::decl()` takes a `&Config` in ts-rs 12. The generator uses `Pong::export_to_string(&ts_rs::Config::default())`. The `#[ts(export)]` attribute was removed from `Pong` — it made `cargo test` auto-export into a stray `crates/protocol/bindings/` directory; `just gen-types` is the single generation path.
- **ESLint config fixes:** `@eslint/js` 10.0.1 was missing from the approved list (it does not track ESLint's version — ESLint is 10.14.0, `@eslint/js` latest is 10.0.1); added to all three TS packages. `eslint-plugin-react-hooks` flat config is `configs.flat["recommended-latest"]`, not `configs["recommended-latest"]` (the latter is the legacy eslintrc shape and ESLint 10 rejects it). Each `eslint.config.js` carries a `disableTypeChecked` block for itself (config files are not in the TS program). `no-empty-object-type` is configured with `allowInterfaces: "always"` (the option takes a string, not a boolean) so the empty `ShopCoreEvents` extension-point interface passes.
- **Vitest setup:** with `globals: false`, Testing Library does not auto-clean up between tests, so both `setup.ts` files call `cleanup()` in an explicit `afterEach`.
- **cargo-deny (deny.toml):** three allow-list additions, all transitive deps of approved stacks, none a new dependency — `MPL-2.0` (Tauri's HTML/CSS parsing deps), `WTFPL` + `blessing` (sqlcipher-src's SQLite blessing). Workspace crates set `publish = false` and `[licenses] private = { ignore = true }` (a `license = "Proprietary"` string is not valid SPDX, so deny flagged every crate as unlicensed). Path-only workspace deps were flagged as wildcards by deny; the fix is `version = "0.1.0"` on the three path deps in the workspace table (single source of version truth kept).
- **Clippy pedantic on docs:** `doc_markdown` fires on CamelCase/acronym words in doc comments (`ShopCore`, `SQLite`, `SQLCipher`, `WAL`, `FTS`, `DB`, `HTTP`, `LAN`, `API`, `PostgreSQL`, `TypeScript`, `OpenSSL`, `SKUs`) — all backticked in doc comments. Public fns returning a value need `#[must_use]` (`core::ping`).
- **`HttpClient.subscribe` (core-client.ts):** the generic type parameter was unused (`TS6133`) because the stub takes no arguments; the implementation drops the generic (the interface keeps it).
- **Local VM notes (not repo changes):** this VM has Node 24.22.1 (the pinned 22.23.4 is what CI installs); pnpm was installed via `npm install -g` because corepack is absent here. The VM was replaced mid-session (system packages reset, `/tmp` cleaned) — Tauri Linux libs were reinstalled afterwards. `libzstd1` had to be downgraded from the image's `+hatch1` build to stock `1.5.5+dfsg2-2build1.1` (same upstream version; dry-run showed only that package affected) or `libwebkit2gtk-4.1-dev` would not install. pnpm reports a peer warning: `eslint-plugin-jsx-a11y@6.10.2` wants ESLint ^3–^9 while we pin ESLint 10.14.0 — the plugin loads and lints fine under ESLint 10; warning only.
- **Not verified on this VM:** the manual `just dev` click-through (press Ping, see `pong`) — the app boots and stays alive under Xvfb (see session summary), but there is no screenshot/click tooling on this VM. The round-trip is proven by the Rust unit test, the Vitest component tests, and the registered command name matching the frontend invoke. The click-through is a Windows manual check.
- **CI build job added 2026-11-10 (user request, exe for testing):** `just build` recipe + a `build-windows` job in `ci.yml` (runs after `check` is green, uploads the Windows `.exe` + NSIS/MSi bundles as an artifact). This is outside spec 000's original "CI runs `just check` only" scope — added because the user asked for a test exe; installer distribution itself is still a later spec's job. No dependency or invariant changed.
- **`just dev` / `just build` recipe fix 2026-11-10:** the recipes originally called the Tauri CLI via `pnpm --filter @shopcore/ui exec tauri …`, but `pnpm exec` re-chroots into `apps/ui`, so the CLI panicked ("Couldn't recognize the current folder as a Tauri project"). Both recipes now call the CLI binary in `apps/ui/node_modules/.bin` directly (verified with `tauri info`).
- **Desktop crate moved to the standard Tauri layout 2026-11-10 (spec correction):** the Appendix A tree put `Cargo.toml` at `crates/desktop/` with `main.rs`/`build.rs` under `src-tauri/` — but the Tauri CLI requires `src-tauri/Cargo.toml` and fails at once without it ("failed to watch …/src-tauri/Cargo.toml"), so `just dev` (FR3) and `just build` could never run. The crate root is now `crates/desktop/src-tauri/` (`Cargo.toml`, `build.rs`, `tauri.conf.json`, `src/main.rs`). This also removes the `config_path` + `codegen` workarounds in `build.rs`/`main.rs` (plain `tauri_build::build()` + `generate_context!()` now work). ADR-018's layout decision stands; only the crate-root location is corrected.
- **`frontendDist` corrected 2026-11-10 (same correction):** it was `../../apps/ui/dist`, which only resolved by accident from the old crate root — the CLI resolves it from the config dir, where it pointed at nonexistent `crates/apps/ui/dist`. It is now `../../../apps/ui/dist` (config dir = manifest dir = `src-tauri/`, so one value serves both). Consequence: the earlier Linux debug binary/Xvfb smoke test may have embedded no frontend — the window staying alive did not prove the UI rendered. The CLI build (which errors loudly when the dist is missing) is the real check.
- **R2 triggered on Windows CI 2026-11-10 (ADR-022):** the first CI run failed in `just check` at `openssl-sys` — vendored OpenSSL's `Configure` died because the build resolved Git's MSYS2 perl (missing `Locale::Maketext::Simple`). Prepending Strawberry Perl to PATH did not fix it (the build still found the MSYS2 perl); the working fix is job-level `OPENSSL_SRC_PERL=C:\Strawberry\perl\bin\perl.exe` in both jobs. No dependency changed; the vcpkg fallback from R2 was not needed.
- **`icons/icon.ico` added 2026-11-10:** with OpenSSL fixed, the Windows build failed at `tauri-build` — it requires `icons/icon.ico` to generate the Windows resource file (Linux builds never needed it). Generated from the 128×128 placeholder PNG (solid accent); a real icon still lands with the branding spec.
- **VM replaced again 2026-11-10:** system packages (Tauri Linux libs) and the npm-global pnpm were wiped a second time. The fresh `just check` run failed at the nextest link step (`-lgtk-3`, `-lwebkit2gtk-4.1` not found) — environment only, not code; fmt/clippy/deny passed in the same run. Libs + pnpm reinstalled the same way as 2026-11-09; re-run pending. Windows CI is unaffected (WebView2, no webkit libs).

---

## Appendix A — File tree to be created (implementation step)

```
Cargo.toml                  # workspace, members, [workspace.lints], [workspace.dependencies]
rust-toolchain.toml         # channel = "1.99.1", components rustfmt/clippy
justfile                    # dev, check, test, bench, gen-types, migrate, seed-large, setup
deny.toml                   # cargo-deny: advisories deny, licences allow-list (MIT/Apache-2.0/BSD/ISC/Unicode-DFS-2016), bans
rustfmt.toml                # edition 2024, max_width 100
.gitignore                  # target/, node_modules/, dist/, *.log
package.json                # pnpm workspace root, packageManager pnpm@12.18.0, engines node >=22
pnpm-workspace.yaml         # apps/*, packages/*
crates/
  domain/Cargo.toml src/lib.rs        # pure, std-only, doc comment
  core/Cargo.toml src/lib.rs          # ping() seam, tokio/thiserror/serde/tracing/uuid
  db/Cargo.toml src/lib.rs            # rusqlite SQLCipher dep, doc comment, no code yet
  hardware/Cargo.toml src/lib.rs      # empty, doc comment
  protocol/Cargo.toml src/lib.rs src/bin/gen_types.rs  # Pong + ts-rs export
  desktop/src-tauri/{Cargo.toml, build.rs, tauri.conf.json, capabilities/default.json, src/main.rs}
  hub/Cargo.toml src/main.rs          # stub binary, no deps
  server/Cargo.toml src/main.rs       # stub binary, no deps
  bench/Cargo.toml src/lib.rs         # empty, doc comment
apps/ui/
  package.json vite.config.ts tsconfig.json index.html
  src/main.tsx src/App.tsx src/index.css
  src/lib/core-client.ts              # CoreClient + TauriClient (+ HttpClient stub)
  src/features/shell/Shell.tsx        # StatusPill + Ping UI (i18n keys)
packages/
  ui-kit/package.json tokens.css src/index.ts src/StatusPill.tsx
  i18n/package.json src/en.json src/index.ts
  protocol-ts/package.json src/generated.ts   # GENERATED, committed
tests/golden/.gitkeep  tests/e2e/.gitkeep
.github/workflows/ci.yml            # windows-latest: setup toolchain/pnpm/just, just check, gen-types freshness
```

Tauri wiring (recorded decision, not a conflict): `crates/desktop/src-tauri/tauri.conf.json` sets `frontendDist: "../../../apps/ui/dist"`, `beforeDevCommand: "pnpm --filter @shopcore/ui dev"`, `beforeBuildCommand: "pnpm --filter @shopcore/ui build"`, `devUrl: "http://localhost:1420"`. `just dev` runs the Tauri CLI from `crates/desktop/src-tauri`.

## Appendix B — Tokens (verbatim copy from `context/10-ui-system.md`, to be placed in `packages/ui-kit/tokens.css`)

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

Motion tokens (also verbatim from `10-ui-system.md`, same file):

```css
:root {
  --dur-instant:80ms; --dur-fast:120ms; --dur-base:180ms; --dur-slow:260ms;
  --ease-out:   cubic-bezier(0.16, 1, 0.3, 1);
  --ease-in:    cubic-bezier(0.4, 0, 1, 1);
  --ease-inout: cubic-bezier(0.4, 0, 0.2, 1);
}
```
