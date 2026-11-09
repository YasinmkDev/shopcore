import type { ReactElement } from "react";
import { Shell } from "./features/shell/Shell";
import { coreClient } from "./lib/core-client";

export function App(): ReactElement {
  return <Shell client={coreClient} />;
}
