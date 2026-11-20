import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Page } from "./Page";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Page user={{ name: "Ada" }} />
  </StrictMode>,
);
