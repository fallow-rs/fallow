import type { Row } from "./db";

export function runTask(task: { run: () => Row }, attempts: number): number {
  if (attempts < 1) return 0;
  return task.run().value;
}
