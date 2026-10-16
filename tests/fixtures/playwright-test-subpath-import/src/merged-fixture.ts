import { mergeTests } from "playwright/test";
import { appTest } from "./app-fixture";
import { ReportsPage } from "./pages/reports-page";

export const test = mergeTests(appTest).extend<{ reports: ReportsPage }>({
  reports: async ({}, use) => {
    await use(new ReportsPage());
  },
});
