import { auditTest } from "../src/helper-combined-fixture";

auditTest()("shows the audit", async ({ audit }) => {
  audit.showAudit();
});
