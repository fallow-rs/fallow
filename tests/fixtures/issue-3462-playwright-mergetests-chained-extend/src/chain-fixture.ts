import { mergeTests } from "@playwright/test";
import { appTest } from "./app-fixture";
import { ChartPage } from "./pages/chart-page";
import { ExportPage } from "./pages/export-page";

export const chainTest = mergeTests(appTest)
  .extend<{ chart: ChartPage }>({
    chart: async ({}, use) => {
      await use(new ChartPage());
    },
  })
  .extend<{ exporter: ExportPage }>({
    exporter: async ({}, use) => {
      await use(new ExportPage());
    },
  });
