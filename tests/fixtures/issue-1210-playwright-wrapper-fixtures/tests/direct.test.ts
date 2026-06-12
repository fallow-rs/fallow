import { testPrimary } from "../primary-fixture";

testPrimary("checks messages through the primary fixture", async ({ app }) => {
  await app.assert.messageChecks.hasExpectedRecord();
  await app.assert.messageChecks.hasMessageForRecordId("ID-123");
});
