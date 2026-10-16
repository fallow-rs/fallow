import { adminTest } from "../src/admin-fixture";
import { test } from "../src/merged-fixture";

test("opens a report", async ({ reports }) => {
  reports.open();
});

adminTest("logs in", async ({ admin }) => {
  admin.login();
});
