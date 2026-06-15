import { usedAction } from "./app/actions";

// Entry point that keeps app/actions.ts reachable and credits usedAction,
// leaving deadAction as a genuine (reachable) unused export.
export async function main() {
  await usedAction();
}
