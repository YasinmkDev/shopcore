import { useEffect, useState } from "react";
import type { ComponentType, ReactElement } from "react";
import { useMotionTier } from "@shopcore/ui-kit";
import type { UiKitTestPageProps } from "./features/dev/UiKitTestPage";
import { BenchDebug } from "./features/bench/BenchDebug";
import { Shell } from "./features/shell/Shell";
import { coreClient } from "./lib/core-client";
import { ThemeProvider } from "./lib/theme";

function AppContent(): ReactElement {
  const [bench, setBench] = useState(false);
  const [openedAtMs, setOpenedAtMs] = useState(0);
  const [DevPage, setDevPage] = useState<ComponentType<UiKitTestPageProps> | null>(null);
  /* Single useMotionTier instance for the whole app (spec 000b, req. 15):
     the dev test page receives it as a prop and never starts a second sampler. */
  const motion = useMotionTier();

  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "b") {
        setOpenedAtMs(performance.now());
        setBench((prev) => !prev);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  /* Dev-only ui-kit test page (spec 000b, req. 13): reachable only in dev
     builds at #ui-kit-test, via a dynamic import. In production builds
     import.meta.env.DEV is false and the branch (with the import) is
     dead-code-eliminated, so the page never ships. */
  useEffect(() => {
    if (import.meta.env.DEV && window.location.hash === "#ui-kit-test") {
      void import("./features/dev/UiKitTestPage").then((m) => {
        setDevPage(() => m.UiKitTestPage);
      });
    }
  }, []);

  if (DevPage !== null) {
    return <DevPage motion={motion} />;
  }
  if (bench) {
    return <BenchDebug openedAtMs={openedAtMs} />;
  }
  return <Shell client={coreClient} />;
}

export function App(): ReactElement {
  return (
    <ThemeProvider>
      <AppContent />
    </ThemeProvider>
  );
}
