import { test } from "../src/combined-fixture";

test("opens a report", async ({ app }) => {
  app.pages.dashboard.openReport();
});
