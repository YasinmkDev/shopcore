import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Tauri expects a fixed port in dev (see tauri.conf.json devUrl).
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "es2022",
    // The e2e build (`VITE_SHOPCORE_E2E=1`) goes to `dist-e2e` so it
    // never overwrites the release bundle `just build` verifies.
    outDir: process.env.VITE_SHOPCORE_E2E === "1" ? "dist-e2e" : "dist",
  },
});
