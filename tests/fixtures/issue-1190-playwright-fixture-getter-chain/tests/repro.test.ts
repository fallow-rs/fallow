import { test } from "../fixture";

test("checks messages through the nested fixture", async ({ app }) => {
  await app.assert.messageChecks.hasExpectedRecord();
  await app.assert.messageChecks.hasMessageForRecordId("ID-123");
});
