/**
 * theme-init.js — sets `data-theme` on <html> BEFORE the first React
 * render, so the first paint uses the right theme (no flash).
 *
 * Loaded by a plain <script src="/theme-init.js"> in index.html, ahead of
 * the module script. It is an EXTERNAL script on purpose: the CSP never
 * allows 'unsafe-inline' in script-src, so this must never become an
 * inline <script> (see the CSP test in src/test/csp.test.ts).
 *
 * Reads the stored setting from localStorage key `shopcore.theme`
 * ("light" | "dark" | "auto"; default "light"). "auto" resolves via
 * matchMedia. localStorage / matchMedia may be unavailable (private
 * mode, old WebView) — every access is guarded and falls back to light.
 * No dependencies, no modules, ES5 syntax (it runs before any bundler
 * output in the oldest supported WebView).
 */
(function () {
  var theme = "light";
  try {
    var stored = window.localStorage.getItem("shopcore.theme");
    if (stored === "light" || stored === "dark" || stored === "auto") {
      theme = stored;
    }
  } catch (e) {
    /* localStorage unavailable — keep the default. */
  }
  if (theme === "auto") {
    try {
      theme = window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light";
    } catch (e) {
      theme = "light";
    }
  }
  document.documentElement.setAttribute("data-theme", theme);
})();
