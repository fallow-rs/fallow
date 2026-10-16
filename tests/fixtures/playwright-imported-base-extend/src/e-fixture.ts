import { appTest } from "./fixtures";
import { EPage } from "./pages/e-page";

export const eTest = appTest.extend<{ e: EPage }>({
  e: async ({}, use) => {
    await use(new EPage());
  },
});
