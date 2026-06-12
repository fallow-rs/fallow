import { extendedMergedTest } from "../extended-merged-fixture";

extendedMergedTest("checks messages through an extended merged wrapper", async ({ app }) => {
  await app.assert.messageChecks.hasExtendedMergedRecord();
});
