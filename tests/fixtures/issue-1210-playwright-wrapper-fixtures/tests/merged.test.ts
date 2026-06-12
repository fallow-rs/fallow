import { mergedTest } from "../merged-fixture";

mergedTest("checks messages through merged fixtures", async ({ app, recordId }) => {
  await app.assert.messageChecks.hasMergedRecord();
  await app.assert.messageChecks.hasMergedMessageForRecordId(recordId);
});
