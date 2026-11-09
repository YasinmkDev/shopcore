import { cleanup } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";

// Testing Library does not auto-clean up when vitest runs with
// globals: false, so unmount between tests explicitly.
afterEach(() => {
  cleanup();
});
