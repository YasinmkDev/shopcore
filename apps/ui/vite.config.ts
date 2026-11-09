import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Tauri expects a fixed port in dev (see tauri.conf.json devUrl).
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "es2022" },
});
