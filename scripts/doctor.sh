#!/usr/bin/env bash
# ShopCore doctor — check the toolchain + system libraries `just check`
# needs, and print what is missing (with the repair command). Read-only:
# it changes nothing. Run: `just doctor` (or `bash scripts/doctor.sh`).
#
# Expected versions are the pinned ones (see AGENTS.md / Cargo.toml):
#   Rust 1.99.1, just 1.59.0, cargo-nextest 0.9.151, cargo-deny 0.20.2,
#   Node 22.23.4, pnpm 12.18.0, libzstd1 1.5.5+dfsg2-2build1.1.
MISSING=0

say() { printf '%s\n' "$*"; }
ok() { say "  OK   $1"; }
bad() { say "  MISS $1"; MISSING=1; }
warn() { say "  WARN $1"; }

check_cmd() { # check_cmd <label> <want-substring> <command...>
    local label="$1" want="$2"; shift 2
    if ! out="$("$@" 2>&1)"; then
        bad "$label — not found (run scripts/vm-repair.sh)"
        return
    fi
    if [ -n "$want" ] && ! printf '%s' "$out" | grep -q "$want"; then
        warn "$label — found but not $want (got: $out)"
        return
    fi
    ok "$label ($out)"
}

say "Toolchain:"
check_cmd "cargo" "1.99.1" cargo --version
check_cmd "rustc" "1.99.1" rustc --version
check_cmd "just" "1.59.0" just --version
check_cmd "cargo-nextest" "0.9.151" cargo-nextest --version
check_cmd "cargo-deny" "0.20.2" cargo-deny --version
check_cmd "node" "v22.23.4" node --version
check_cmd "pnpm" "12.18.0" pnpm --version

say "System libraries (desktop crate links against these):"
for pkg in gtk+-3.0 webkit2gtk-4.1 javascriptcoregtk-4.1 libsoup-3.0 \
    gdk-pixbuf-2.0 atk dbus-1 ayatana-appindicator3-0.1 librsvg-2.0; do
    if pkg-config --exists "$pkg" 2>/dev/null; then
        ok "pkg-config $pkg"
    else
        bad "pkg-config $pkg — install it (run scripts/vm-repair.sh)"
    fi
done

say "libzstd1 (the +hatch1 build breaks libzstd-dev):"
zstd_ver="$(dpkg-query -W -f='${Version}' libzstd1 2>/dev/null || echo missing)"
if [ "$zstd_ver" = "1.5.5+dfsg2-2build1.1" ]; then
    ok "libzstd1 $zstd_ver"
else
    bad "libzstd1 is '$zstd_ver', want 1.5.5+dfsg2-2build1.1 (run scripts/vm-repair.sh)"
fi

say "PostgreSQL (server chaos tests only; just check excludes them):"
if command -v psql >/dev/null 2>&1; then
    ok "psql ($(psql --version | head -1))"
else
    warn "psql not found — server chaos tests cannot run locally (run scripts/vm-repair.sh)"
fi

if [ "$MISSING" -eq 0 ]; then
    say "doctor: everything present."
else
    say "doctor: something is missing — run: sudo ./scripts/vm-repair.sh"
    exit 1
fi
