"use server";

// Referenced (import-and-call) from page.tsx, so the module is reachable.
export async function usedAction() {
  await persist();
}

// A dead export of a "use server" file. Under Next this would be reclassified to
// `unused-server-action`, but Server Action REGISTRATION is a Next-specific
// concept, so the rule stays Next-gated: under Waku this stays a plain
// `unused-export` and the `unused_server_actions` bucket must be empty.
export async function deadAction() {
  await persist();
}

async function persist() {}
