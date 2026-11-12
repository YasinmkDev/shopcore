/**
 * Playwright global setup (spec 001c-2): spawn the real desktop
 * binary and a `vite preview` server for the e2e UI build, then
 * write their addresses to `e2e/auth.json` for the tests.
 *
 * The desktop is built by the `just e2e` recipe (or the CI e2e job)
 * BEFORE playwright runs — `tauri build --features
 * e2e-shim,dev-session --config tauri.dev.conf.json` — so this file
 * only spawns it and fails clearly when the binary is missing.
 *
 * Fresh data per run: the dev seed is a fresh-DB-only operation
 * (calling it twice fails on the unique constraints), so the desktop
 * gets a fresh data dir — on Linux via `XDG_DATA_HOME` pointed at
 * `e2e/.tmp-data`, on Windows by removing the dev app-data dir.
 * On Linux the desktop is wrapped in `xvfb-run -a` (no real display
 * on this VM or in CI); on Windows the runner has a desktop session.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { promises as fs } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const UI_DIR = resolve(fileURLToPath(new URL(".", import.meta.url)), "..");
const E2E_DIR = join(UI_DIR, "e2e");
const PREVIEW_PORT = 4173;

export interface E2eAuth {
  previewUrl: string;
  shimUrl: string;
  token: string;
}

interface ProcessRecord {
  desktopPid: number | undefined;
  previewPid: number | undefined;
}

function desktopExePath(): string {
  const fromEnv = process.env.SHOPCORE_E2E_EXE;
  if (fromEnv !== undefined && fromEnv !== "") {
    return fromEnv;
  }
  const name = process.platform === "win32" ? "shopcore-desktop.exe" : "shopcore-desktop";
  // The desktop is a workspace member, so cargo puts the binary in
  // the WORKSPACE target dir (`<repo>/target/release`), not under
  // `crates/desktop/src-tauri/target` — the per-crate path does not
  // exist and the setup failed there before any test ran.
  return join(UI_DIR, "..", "..", "target", "release", name);
}

function waitForOutput(
  child: ChildProcess,
  pattern: RegExp,
  timeoutMs: number,
  what: string,
): Promise<string> {
  return new Promise((resolveLine, reject) => {
    let buffer = "";
    const timer = setTimeout(() => {
      reject(new Error(`timed out waiting for ${what} from the desktop (got: ${buffer})`));
    }, timeoutMs);
    child.stdout?.on("data", (chunk: Buffer) => {
      buffer += chunk.toString("utf8");
      const match = pattern.exec(buffer);
      if (match?.[1] !== undefined) {
        clearTimeout(timer);
        resolveLine(match[1]);
      }
    });
    child.on("error", (err) => {
      clearTimeout(timer);
      reject(err);
    });
    child.on("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`desktop exited with ${String(code)} while waiting for ${what}`));
    });
  });
}

async function waitForHttp(url: string, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try {
      const response = await fetch(url);
      if (response.ok) {
        return;
      }
    } catch {
      // Not up yet.
    }
    if (Date.now() > deadline) {
      throw new Error(`timed out waiting for ${url}`);
    }
    await new Promise((resolveSleep) => setTimeout(resolveSleep, 250));
  }
}

async function setup(): Promise<void> {
  const exe = desktopExePath();
  try {
    await fs.access(exe);
  } catch {
    throw new Error(
      `e2e desktop binary not found at ${exe} — run \`just e2e\` (it builds it) instead of \`playwright test\` directly`,
    );
  }

  // Fresh data dir: the dev seed only runs on a fresh DB.
  const dataRoot = join(E2E_DIR, ".tmp-data");
  await fs.rm(dataRoot, { recursive: true, force: true });
  await fs.mkdir(dataRoot, { recursive: true });
  const desktopEnv: NodeJS.ProcessEnv = { ...process.env };
  if (process.platform === "linux") {
    desktopEnv.XDG_DATA_HOME = dataRoot;
  } else if (process.platform === "win32") {
    const appData = process.env.APPDATA;
    if (appData !== undefined) {
      await fs.rm(join(appData, "com.shopcore.desktop.dev"), {
        recursive: true,
        force: true,
      });
    }
  }

  const command = process.platform === "linux" ? "xvfb-run" : exe;
  const args = process.platform === "linux" ? ["-a", exe] : [];
  const desktop = spawn(command, args, {
    env: desktopEnv,
    stdio: ["ignore", "pipe", "inherit"],
  });
  // If anything below fails, kill the children we already spawned:
  // a failed setup otherwise orphans the desktop + preview (the
  // preview keeps port 4173 and the next run cannot start it).
  let preview: ChildProcess | undefined;
  try {
    await startPreviewAndWriteAuth(desktop);
  } catch (err) {
    killChild(desktop, "desktop");
    if (preview !== undefined) {
      killChild(preview, "preview");
    }
    throw err;
  }

  async function startPreviewAndWriteAuth(
    desktopChild: ChildProcess,
  ): Promise<void> {
    // Both listeners attach BEFORE awaiting: the three startup lines
    // arrive together, and a sequential await could miss the token.
    const portPromise = waitForOutput(
      desktopChild,
      /SHOPCORE_E2E_PORT=(\d+)/,
      60_000,
      "the shim port line",
    );
    const tokenPromise = waitForOutput(
      desktopChild,
      /SHOPCORE_E2E_TOKEN=(\S+)/,
      60_000,
      "the shim token line",
    );
    const [port, token] = await Promise.all([portPromise, tokenPromise]);

    // The e2e UI build (`VITE_SHOPCORE_E2E=1` → `dist-e2e`) served
    // over HTTP; the test passes the shim URL + token as query params.
    // `--host 127.0.0.1`: vite's default `localhost` binds ::1 only
    // in this environment, and the harness fetches 127.0.0.1.
    preview = spawn(
      "pnpm",
      [
        "exec",
        "vite",
        "preview",
        "--host",
        "127.0.0.1",
        "--port",
        String(PREVIEW_PORT),
        "--strictPort",
      ],
      {
        cwd: UI_DIR,
        env: { ...process.env, VITE_SHOPCORE_E2E: "1" },
        stdio: ["ignore", "pipe", "inherit"],
      },
    );
    const previewUrl = `http://127.0.0.1:${String(PREVIEW_PORT)}`;
    await waitForHttp(previewUrl, 30_000);

    const auth: E2eAuth = {
      previewUrl,
      shimUrl: `http://127.0.0.1:${port}`,
      token,
    };
    await fs.writeFile(join(E2E_DIR, "auth.json"), JSON.stringify(auth));
    const record: ProcessRecord = {
      desktopPid: desktopChild.pid,
      previewPid: preview.pid,
    };
    await fs.writeFile(join(E2E_DIR, ".processes.json"), JSON.stringify(record));
    // Detach the children from this process's lifetime: the teardown
    // kills them by pid. Without this, a setup failure would orphan
    // the desktop (it would keep the shim port + the data dir).
    desktopChild.unref();
    preview.unref();
  }
}

function killChild(child: ChildProcess, name: string): void {
  if (child.pid === undefined) {
    return;
  }
  try {
    process.kill(child.pid, "SIGTERM");
  } catch {
    // Already gone — nothing to stop.
    return;
  }
  console.log(`e2e setup: killed orphaned ${name} (pid ${String(child.pid)})`);
}

export default setup;
