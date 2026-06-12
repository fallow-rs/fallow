import { extendedTest } from "../extended-fixture";

extendedTest("checks messages through an extended wrapper", async ({ app }) => {
  await app.assert.messageChecks.hasExtendedRecord();
  await app.assert.messageChecks.hasExtendedMessageForRecordId("ID-456");
});
