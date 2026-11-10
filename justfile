# ShopCore justfile. Command contract: see AGENTS.md.
# `just check` is defined once, in AGENTS.md:
#   fmt + clippy (-D warnings) + cargo deny + nextest + tsc + eslint + vitest.
# Recipes are single commands per line so they run under both cmd.exe
# (Windows CI) and sh (Linux/macOS dev machines). No shell-specific syntax.

# Install the pinned toolchains, then fetch dependencies.
setup:
    rustup toolchain install 1.99.1 --profile minimal --component rustfmt clippy
    corepack enable
    corepack prepare pnpm@12.18.0 --activate
    pnpm install
    cargo fetch

# Run the desktop app with hot reload (Tauri dev server, from the crate root).
# NOTE: the Tauri CLI is called by its node_modules/.bin path, NOT via
# `pnpm --filter @shopcore/ui exec` — pnpm exec re-chroots to apps/ui and
# the CLI then cannot find tauri.conf.json (hit 2026-11-10).
[working-directory: 'crates/desktop/src-tauri']
dev:
    ../../../apps/ui/node_modules/.bin/tauri dev

# Build the desktop app for the current OS (Tauri build, from the crate root).
# On Windows CI this produces target/release/shopcore-desktop.exe plus the
# MSI/NSIS bundles. Added 2026-11-10 at the user's request (exe for testing);
# installer distribution is otherwise a later spec's job.
[working-directory: 'crates/desktop/src-tauri']
build:
    ../../../apps/ui/node_modules/.bin/tauri build

# Full gate. Must exit 0 before any work is considered done.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo deny check
    # The server chaos tests need a live Postgres (they fail loudly
    # without one, per spec 005 amendment 5) and run only in the Linux
    # `server-tests` CI job (ADR-015) and locally via
    # `cargo nextest run -p shopcore-server --test chaos`. Exclude just
    # that test binary here so future server unit tests still run.
    cargo nextest run --workspace --filter-expr "not binary(chaos)"
    pnpm install --frozen-lockfile
    pnpm --filter @shopcore/ui run typecheck
    pnpm --filter @shopcore/ui-kit run typecheck
    pnpm --filter @shopcore/i18n run typecheck
    pnpm --filter @shopcore/protocol-ts run typecheck
    pnpm --filter @shopcore/ui run lint
    pnpm --filter @shopcore/ui-kit run lint
    pnpm --filter @shopcore/i18n run lint
    pnpm --filter @shopcore/ui run test
    pnpm --filter @shopcore/ui-kit run test

# Tests only (Rust + TypeScript).
test:
    # Same exclusion as `check`: the chaos tests need Postgres and run
    # in the Linux `server-tests` CI job (ADR-015).
    cargo nextest run --workspace --filter-expr "not binary(chaos)"
    pnpm --filter @shopcore/ui run test
    pnpm --filter @shopcore/ui-kit run test

# Performance benchmarks against the budgets in 05-performance-budgets.md.
# Full gate (spec 002): runs all scenarios on the seeded database and
# writes target/bench/results/results.json + results.md. Manual run on the
# target PC; dev-machine numbers are PROVISIONAL. Run `just seed-large`
# first (the runner errors clearly if the database is missing).
bench:
    cargo run --release -p shopcore-bench -- run --size full --db target/bench/bench.db --out target/bench/results

# CI smoke: seeds the 10k-row dataset, runs the scenarios, compares against
# the stored baseline (relative tripwire: fails only on >2x regression).
# Not an absolute budget check (05, amendment 2). --fresh: CI always
# starts from a clean database, never resumes a stale one.
bench-smoke:
    cargo run --release -p shopcore-bench -- seed --size smoke --db target/bench/bench-smoke.db --fresh
    cargo run --release -p shopcore-bench -- run --size smoke --db target/bench/bench-smoke.db --out target/bench/smoke-results

# Refresh the stored CI smoke baseline deliberately (never automatic).
bench-update-baseline:
    cargo run --release -p shopcore-bench -- run --size smoke --db target/bench/bench-smoke.db --out target/bench/smoke-results --update-baseline

# Regenerate TypeScript types from Rust into packages/protocol-ts.
# Never hand-edit the generated file.
gen-types:
    cargo run -p shopcore-protocol --bin gen_types

# Apply local DB migrations. Stub in spec 000: no migrations exist yet.
migrate:
    echo no migrations yet

# Seed 500k SKUs / 5M sale lines for benchmarking (spec 002).
# Resumes an interrupted seed by default; pass --fresh to start over.
seed-large:
    cargo run --release -p shopcore-bench -- seed --size full --db target/bench/bench.db

# Seed the 10k-row CI smoke dataset (spec 002).
# Resumes an interrupted seed by default; pass --fresh to start over.
seed-smoke:
    cargo run --release -p shopcore-bench -- seed --size smoke --db target/bench/bench-smoke.db

# Prove a seeded database: checksum + per-SKU balance == sum(movements).
bench-verify:
    cargo run --release -p shopcore-bench -- verify --db target/bench/bench.db

# Build the standalone bench exe (release) for the Windows target-PC run.
# CI uploads it as the `shopcore-bench-windows` artifact with RUN.md.
bench-exe:
    cargo build --release -p shopcore-bench
