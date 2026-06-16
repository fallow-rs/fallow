import { testMerged } from "../fixtures";
import type { MergedReader } from "../reader";

testMerged("merged fixture branch aliases", async ({ readerA, readerB }) => {
  await readerA.directControl();

  const ternaryReader = process.env.READER_KIND === "a" ? readerA : readerB;
  await ternaryReader.ternarySelected();

  let ifReader: MergedReader;
  if (process.env.READER_KIND === "a") {
    ifReader = readerA;
    await readerA.ifElseDirect();
  } else {
    ifReader = readerB;
    await readerB.ifElseDirect();
  }
  await ifReader.ifElseSelected();

  let switchReader: MergedReader;
  switch (process.env.READER_KIND) {
    case "a":
      switchReader = readerA;
      await readerA.switchDirect();
      break;
    default:
      switchReader = readerB;
      await readerB.switchDirect();
      break;
  }
  await switchReader.switchSelected();
});
