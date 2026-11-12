#!/usr/bin/env node
/**
 * verify-release-bundle.mjs (spec 001c-2): the release UI bundle
 * must NOT contain the e2e shim. The `VITE_SHOPCORE_E2E` flag gates
 * the `HttpClient` import in `core-client.ts`, so Rollup tree-shakes
 * it out of the release build — this script proves it by grepping the
 * built bundle in `apps/ui/dist` for the shim markers:
 * - `/__e2e/` — the shim URL the HttpClient fetches;
 * - `x-shopcore-e2e-token` — the shim's auth header;
 * - `SHOPCORE_E2E_TOKEN` — the token plumbing.
 *
 * Usage: `node apps/ui/scripts/verify-release-bundle.mjs`
 * (also the `check-release-bundle` just recipe). Exits non-zero
 * when a marker is found, listing the file + marker.
 */
import { readdir, readFile, stat } from "node:fs/promises";
import { join } from "node:path";

const DIST = new URL("../dist", import.meta.url);
const MARKERS = ["/__e2e/", "x-shopcore-e2e-token", "SHOPCORE_E2E_TOKEN"];

async function* files(dir) {
  for (const entry of await readdir(dir)) {
    const path = join(dir.pathname, entry);
    const info = await stat(path);
    if (info.isDirectory()) {
      yield* files(new URL(`file://${path}/`));
    } else {
      yield path;
    }
  }
}

let found = 0;
for await (const path of files(DIST)) {
  const text = await readFile(path, "utf8").catch(() => null);
  if (text === null) {
    continue;
  }
  for (const marker of MARKERS) {
    if (text.includes(marker)) {
      console.error(`release bundle contains shim marker ${JSON.stringify(marker)}: ${path}`);
      found += 1;
    }
  }
}

if (found > 0) {
  console.error(`verify-release-bundle: ${String(found)} shim marker(s) in the release bundle`);
  process.exit(1);
}
console.log("verify-release-bundle: no shim markers in the release bundle");
