import { bTest } from "./b-fixture";
import { DPage } from "./pages/d-page";

export const dTest = bTest.extend<{ d: DPage }>({
  d: async ({}, use) => {
    await use(new DPage());
  },
});
