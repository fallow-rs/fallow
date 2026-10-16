import { plainTest } from "./plain-fixture";
import { CPage } from "./pages/c-page";

export const cTest = plainTest.extend<{ c: CPage }>({
  c: async ({}, use) => {
    await use(new CPage());
  },
});
