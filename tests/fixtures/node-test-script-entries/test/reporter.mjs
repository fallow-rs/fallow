import { describeEvent } from "../src/report-format.mjs";

export default async function* reporter(source) {
  for await (const event of source) {
    yield `${describeEvent(event)}\n`;
  }
}
