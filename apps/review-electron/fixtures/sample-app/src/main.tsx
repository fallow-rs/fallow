import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

// Dev only: inject the Fallow grounded-inspector picker, posting selections to
// the Electron host's localhost bridge.
if (import.meta.env.DEV) {
  void import("../../../src/inspector/picker").then((m) =>
    m.startInspector({ bridgeUrl: "http://localhost:7787/fallow-select" }),
  );
}
