import { appTest } from "./app-fixture";
import { HelperPage } from "./pages/helper-page";

export const makeHelperTest = () =>
  appTest.extend<{ helper: HelperPage }>({
    helper: async ({}, use) => {
      await use(new HelperPage());
    },
  });
