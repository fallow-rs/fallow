import { runMochaSuite } from "../suite/index.js";

export async function run(): Promise<void> {
  await runMochaSuite(__dirname);
}
