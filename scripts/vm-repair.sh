#!/usr/bin/env bash
# ShopCore VM repair — restore the toolchain + system libraries that a VM
# replacement wipes. System packages and the global pnpm install do NOT
# survive a VM replacement (the home directory does, so cargo/rustup/just
# and the repo survive). Idempotent — safe to re-run.
#
# The exact steps used 2026-11-11 when `just check` failed after a VM
# reset with `pnpm: not found` and missing GTK/WebKit link libraries.
#
# Usage: run as root, or with sudo:
#   sudo ./scripts/vm-repair.sh
set -euo pipefail

# apt needs root; re-exec under sudo when available.
if [ "$(id -u)" -ne 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
        exec sudo "$0" "$@"
    fi
    echo "vm-repair: need root (apt + global npm install). Re-run with sudo." >&2
    exit 1
fi

# 1. Apt sources: the azure mirror HANGS on this VM — re-point to the
#    main Ubuntu archive (the recurring repair; the file is reverted by
#    a VM replacement, so this step runs every time).
if grep -q "azure.archive.ubuntu.com\|mirror.cogentco.com" /etc/apt/sources.list.d/ubuntu.sources 2>/dev/null; then
    echo "vm-repair: re-pointing apt sources to archive.ubuntu.com ..."
    sed -i 's|http://azure.archive.ubuntu.com/ubuntu|http://archive.ubuntu.com/ubuntu|g; s|http://mirror.cogentco.com/pub/linux/ubuntu|http://archive.ubuntu.com/ubuntu|g' /etc/apt/sources.list.d/ubuntu.sources
fi
apt-get update

# 2. libzstd1: the VM ships a patched `+hatch1` build that breaks
#    libzstd-dev (unmet dependency). Downgrade to the stock Ubuntu build.
INSTALLED_ZSTD="$(dpkg-query -W -f='${Version}' libzstd1 2>/dev/null || true)"
if [ "$INSTALLED_ZSTD" != "1.5.5+dfsg2-2build1.1" ]; then
    echo "vm-repair: downgrading libzstd1 ($INSTALLED_ZSTD -> 1.5.5+dfsg2-2build1.1) ..."
    apt-get install -y --allow-downgrades libzstd1=1.5.5+dfsg2-2build1.1
fi

# 3. GTK/WebKit dev libraries: the desktop crate (Tauri/WebView2 on
#    Linux) does not link without them (`unable to find library -lgtk-3`
#    etc. at `just check`). Also the Xvfb smoke-test helpers.
echo "vm-repair: installing GTK/WebKit dev libraries ..."
apt-get install -y \
    libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev libdbus-1-dev \
    libatk1.0-dev libgdk-pixbuf-2.0-dev libayatana-appindicator3-dev \
    librsvg2-dev patchelf imagemagick xdotool openbox

# 4. pnpm: the global install is wiped by a VM replacement.
#    (`just check` fails with `pnpm: not found` without it.)
if ! command -v pnpm >/dev/null 2>&1; then
    echo "vm-repair: installing pnpm@12.18.0 ..."
    npm install -g pnpm@12.18.0
else
    echo "vm-repair: pnpm already present ($(pnpm --version))."
fi

# 5. PostgreSQL 16 (only needed for the server chaos tests, which run in
#    the Linux `server-tests` CI job and locally via
#    `cargo nextest run -p shopcore-server --test chaos`). Skipped when
#    Postgres is already installed and the shopcore databases exist.
if ! command -v psql >/dev/null 2>&1; then
    echo "vm-repair: installing postgresql-16 (server chaos tests need it) ..."
    apt-get install -y postgresql-16
fi
if sudo -u postgres psql -tAc "SELECT 1 FROM pg_roles WHERE rolname='shopcore'" 2>/dev/null | grep -q 1; then
    echo "vm-repair: postgres role 'shopcore' already exists."
else
    echo "vm-repair: creating postgres role 'shopcore' + databases ..."
    sudo -u postgres psql -c "CREATE ROLE shopcore LOGIN SUPERUSER PASSWORD 'shopcore';" 2>/dev/null || true
    sudo -u postgres createdb -O shopcore shopcore 2>/dev/null || true
    sudo -u postgres createdb -O shopcore shopcore_test 2>/dev/null || true
fi

echo "vm-repair: done. Run 'just doctor' to verify, then 'just check'."
