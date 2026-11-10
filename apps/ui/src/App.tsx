import { useEffect, useState } from "react";
import type { ReactElement } from "react";
import { BenchDebug } from "./features/bench/BenchDebug";
import { Shell } from "./features/shell/Shell";
import { coreClient } from "./lib/core-client";

export function App(): ReactElement {
  const [bench, setBench] = useState(false);
  const [openedAtMs, setOpenedAtMs] = useState(0);

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

  if (bench) {
    return <BenchDebug openedAtMs={openedAtMs} />;
  }
  return <Shell client={coreClient} />;
}
