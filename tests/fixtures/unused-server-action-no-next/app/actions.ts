"use server";

// Reachable (imported by consumer.ts), but `deadAction` is referenced nowhere.
// Without `next` declared, the unused-server-action rule must not fire, so
// `deadAction` surfaces as a plain unused-export instead.
export async function usedAction() {
  return 0;
}

export async function deadAction() {
  return 1;
}
