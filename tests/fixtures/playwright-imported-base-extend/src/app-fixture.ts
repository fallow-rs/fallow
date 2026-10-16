import { test as base } from "@playwright/test";
import { AppPage } from "./pages/app-page";

export const appTest = base.extend<{ app: AppPage }>({
  app: async ({}, use) => {
    await use(new AppPage());
  },
});
