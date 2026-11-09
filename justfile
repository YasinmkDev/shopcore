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

# Run the desktop app with hot reload (Tauri dev server, from crates/desktop).
[working-directory: 'crates/desktop']
dev:
    pnpm --filter @shopcore/ui exec tauri dev

# Build the desktop app for the current OS (Tauri build, from crates/desktop).
# On Windows CI this produces target/release/shopcore-desktop.exe plus the
# MSI/NSIS bundles. Added 2026-11-10 at the user's request (exe for testing);
# installer distribution is otherwise a later spec's job.
[working-directory: 'crates/desktop']
build:
    pnpm --filter @shopcore/ui exec tauri build

# Full gate. Must exit 0 before any work is considered done.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo deny check
    cargo nextest run --workspace
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
    cargo nextest run --workspace
    pnpm --filter @shopcore/ui run test
    pnpm --filter @shopcore/ui-kit run test

# Performance benchmarks against the budgets in 05-performance-budgets.md.
# Stub in spec 000: crates/bench is an empty crate. Real benchmarks land
# with the Phase 0 benchmark spec.
bench:
    echo no benchmarks yet

# Regenerate TypeScript types from Rust into packages/protocol-ts.
# Never hand-edit the generated file.
gen-types:
    cargo run -p shopcore-protocol --bin gen_types

# Apply local DB migrations. Stub in spec 000: no migrations exist yet.
migrate:
    echo no migrations yet

# Seed 500k SKUs / 5M sale lines for benchmarking. Stub in spec 000.
seed-large:
    echo no seeder yet
