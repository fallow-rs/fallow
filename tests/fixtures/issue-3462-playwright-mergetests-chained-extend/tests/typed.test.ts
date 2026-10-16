import { typedTest } from "../src/typed-combined-fixture";

typedTest("exports a report", async ({ reports }) => {
  reports.exportReport();
});
