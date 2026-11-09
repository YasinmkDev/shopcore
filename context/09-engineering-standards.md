# 09 — Engineering Standards

## General
- Small vertical slices. One task per session. PRs < ~400 changed lines; split otherwise.
- Conventional commits (`feat:`, `fix:`, `perf:`, `test:`, `refactor:`, `docs:`, `chore:`). One logical change per commit.
- Do not add dependencies without asking. When approved, pin exact versions and record the reason in `08-decisions.md`. Never bump versions as a side effect.
- Write the simplest thing that meets the spec and budgets. No speculative abstractions, no dead code, no commented-out code.
- Comments explain why, not what. Public functions in `domain` and `core` have doc comments with an example for business rules.

## Rust
- Stable toolchain pinned in `rust-toolchain.toml`. `cargo fmt` and `clippy -D warnings` (with `clippy::pedantic` selectively enabled, `unwrap_used`, `expect_used`, `panic`, `indexing_slicing` denied outside tests).
- Errors: `thiserror` enums per module; `anyhow` only in binaries' `main`. Map to `AppError` with stable codes at the boundary.
- Newtypes for domain values: `Money`, `Qty`, `Rate`, `UnitPrice`, `UnitCost`, `SkuId`, `BranchId`, etc. No raw `i64` for money in function signatures.
- Arithmetic on money/qty uses checked operations; overflow is an error, not a wrap.
- No blocking calls on the async runtime; DB work goes to the DB actor; CPU-heavy work to `spawn_blocking`.
- Time, randomness, and IDs are injected via traits (`Clock`, `IdGen`) so tests are deterministic.
- `unsafe` is forbidden without an ADR.
- Public API of each crate is small and documented; modules are private by default.
- Tests: unit tests next to code, property tests (`proptest`) for domain invariants, integration tests in `tests/`. Use `cargo nextest`.

### Pattern: money type (illustrative, finalize in code)
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Money(i64); // minor units
impl Money {
    pub fn checked_add(self, o: Money) -> Result<Money, MoneyError> { /* ... */ }
    pub fn mul_rate_round(self, r: Rate) -> Result<Money, MoneyError> { /* half away from zero */ }
}
```

### Pattern: Tauri command
Thin: deserialize → permission check → call use case → map error. No business logic in commands.
```rust
#[tauri::command]
#[specta::specta]
async fn complete_sale(state: State<'_, Core>, req: CompleteSaleReq) -> Result<SaleReceipt, AppError> {
    state.auth.require(&req.actor, Permission::SaleCreate)?;
    state.sales.complete(req).await
}
```

## TypeScript / React
- `strict: true`, `noUncheckedIndexedAccess`, no `any`, no non-null `!` without justification, no `@ts-ignore`.
- Generated types from Rust live in `packages/protocol-ts` (generated; never edit).
- Feature-based folders: `apps/ui/src/features/<feature>/{components,hooks,state,routes}`.
- Server state via TanStack Query (query keys per resource); UI-only state via Zustand; forms via react-hook-form (validation messages from i18n; authoritative validation in Rust).
- Components: function components, small, typed props, no business logic (call hooks → `CoreClient`). Use only components from `packages/ui-kit` (see `10-ui-system`); add new ones to the registry first.
- Styling only through Tailwind classes mapped to tokens. No hard-coded colors, spacing, font sizes, or durations.
- i18n via keys (`t('sale.total')`); no string literals in JSX; logical CSS properties (`margin-inline-start`) so RTL works.
- ESLint (incl. `jsx-a11y`, `react-hooks`) + Prettier. Zero warnings.
- Tests: Vitest + Testing Library for components/hooks; Playwright for flows (see `11-test-strategy`).

### Pattern: CoreClient
```ts
export interface CoreClient {
  invoke<K extends keyof Commands>(cmd: K, args: Commands[K]['args']): Promise<Commands[K]['result']>;
  subscribe<E extends keyof Events>(event: E, cb: (p: Events[E]) => void): () => void;
}
// implementations: TauriClient (embedded), HttpClient (hub/browser tests)
```

### Pattern: virtualized list
Use `useVirtualizer` with fixed row height, overscan ≤ 10, keyset-paged infinite query, row component wrapped in `memo`.

## Commands and CI
- `just check` (fmt, clippy, nextest, `cargo deny`, tsc, eslint, vitest) must be green before any commit is considered done.
- CI on every push: `just check`, migration tests (previous-release DB → current), `just bench` (budget gate), installer build, `pnpm audit`.
- Release: tagged build → signed installers → staged rollout.

## Documentation
Keep context files truthful. When code changes a documented rule, update the file in the same commit. ADR for any decision a future engineer would ask "why?" about.

## Agent-specific working rules
- Prefer editing existing files and following existing patterns over inventing new ones.
- Before coding, state the plan and the tests you will add. After coding, state what you ran and the results.
- If you hit a spec gap, stop and ask; do not fill it with a guess in business logic.
- Never weaken or delete a test to make it pass. If a test is wrong, explain why and ask.
- Never mock the thing under test. Mock only hardware, clock, network.
