/**
 * Normalize an error thrown across the Electron IPC boundary into a clean,
 * user-facing message. `ipcRenderer.invoke` wraps a handler error as
 * `Error invoking remote method 'review:get': Error: <message>`; the main
 * process already produces native-quality messages, so here we just strip the
 * wrapper noise the IPC layer adds.
 */
export const errorMessage = (e: unknown): string => {
  const raw = e instanceof Error ? e.message : String(e);
  const cleaned = raw
    .replace(/^Error invoking remote method '[^']*':\s*/i, "")
    .replace(/^(?:[A-Za-z]*Error:\s*)+/, "")
    .trim();
  return cleaned || "Something went wrong.";
};
