import { testSingle } from "../fixtures";
import type { SingleReader } from "../reader";

testSingle("single fixture branch aliases", async ({ readerA, readerB }) => {
  await readerA.directControl();

  const ternaryReader = process.env.READER_KIND === "a" ? readerA : readerB;
  await ternaryReader.ternarySelected();

  let ifReader: SingleReader;
  if (process.env.READER_KIND === "a") {
    ifReader = readerA;
  } else {
    ifReader = readerB;
  }
  await ifReader.ifElseSelected();

  let switchReader: SingleReader;
  switch (process.env.READER_KIND) {
    case "a":
      switchReader = readerA;
      break;
    default:
      switchReader = readerB;
      break;
  }
  await switchReader.switchSelected();
});
