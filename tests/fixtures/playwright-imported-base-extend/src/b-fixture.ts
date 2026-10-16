import { appTest } from "./app-fixture";
import { BPage } from "./pages/b-page";

export const bTest = appTest.extend<{ b: BPage }>({
  b: async ({}, use) => {
    await use(new BPage());
  },
});
