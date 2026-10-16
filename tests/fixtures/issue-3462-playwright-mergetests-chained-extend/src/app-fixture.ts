import { test as base } from "@playwright/test";
import { DashboardPage } from "./pages/dashboard-page";

type App = {
  pages: {
    dashboard: DashboardPage;
  };
};

type AppFixtures = {
  app: App;
};

export const appTest = base.extend<AppFixtures>({
  app: async ({}, use) => {
    await use({
      pages: {
        dashboard: new DashboardPage(),
      },
    });
  },
});
