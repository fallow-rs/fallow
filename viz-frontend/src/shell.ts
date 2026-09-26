/**
 * `fallow viz` writes `shell.html` into the page before the payload and the
 * script, so the page shows its frame before the script runs. The shell uses
 * the row ids below and the same stylesheet as the app, so each row takes the
 * same box before and after the script builds the real rows.
 */

/** Ids of the page rows, in page order. */
export const ROW_IDS = {
  topbar: "topbar",
  toolbar: "toolbar",
  summary: "lens-summary",
  stage: "stage",
  statusline: "statusline",
} as const;

/** The token in `shell.html` that the CLI replaces with the project name. */
export const ROOT_PLACEHOLDER = "__FALLOW_ROOT__";

/** Remove the static shell and give the app an empty root element. */
export const mountApp = (doc: Document): HTMLElement => {
  doc.getElementById("app")?.remove();
  const app = doc.createElement("div");
  app.id = "app";
  doc.body.appendChild(app);
  return app;
};
