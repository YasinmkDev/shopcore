/**
 * Playwright global teardown (spec 001c-2): kill the desktop and
 * the `vite preview` server the global setup spawned (by the pids
 * in `e2e/.processes.json`), then remove the per-run files.
 */
import { promises as fs } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const E2E_DIR = resolve(fileURLToPath(new URL(".", import.meta.url)));

async function kill(pid: number | undefined, name: string): Promise<void> {
  if (pid === undefined) {
    return;
  }
  try {
    process.kill(pid, "SIGTERM");
  } catch {
    // Already gone — nothing to stop.
    return;
  }
  // Give it a moment; the desktop's shutdown joins its threads.
  for (let i = 0; i < 50; i += 1) {
    try {
      process.kill(pid, 0);
    } catch {
      return;
    }
    await new Promise((resolveSleep) => setTimeout(resolveSleep, 100));
  }
  try {
    process.kill(pid, "SIGKILL");
  } catch {
    // Gone by now.
  }
  console.log(`e2e teardown: ${name} (pid ${String(pid)}) did not stop, SIGKILL sent`);
}

async function teardown(): Promise<void> {
  let record: { desktopPid?: number; previewPid?: number } = {};
  try {
    record = JSON.parse(
      await fs.readFile(join(E2E_DIR, ".processes.json"), "utf8"),
    ) as { desktopPid?: number; previewPid?: number };
  } catch {
    // No record — nothing was spawned (setup failed before spawn).
  }
  await Promise.all([
    kill(record.previewPid, "vite preview"),
    kill(record.desktopPid, "desktop"),
  ]);
  await fs.rm(join(E2E_DIR, "auth.json"), { force: true });
  await fs.rm(join(E2E_DIR, ".processes.json"), { force: true });
  await fs.rm(join(E2E_DIR, ".tmp-data"), { recursive: true, force: true });
}

export default teardown;
