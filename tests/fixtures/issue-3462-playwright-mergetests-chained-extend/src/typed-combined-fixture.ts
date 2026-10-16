import { mergeTests as merge } from "@playwright/test";
import { appTest } from "./app-fixture";
import { ReportsPage } from "./pages/reports-page";

type ReportFixtures = {
  reports: ReportsPage;
};

export const typedTest = merge(appTest).extend<ReportFixtures>({
  reports: async ({}, use) => {
    await use(new ReportsPage());
  },
});
