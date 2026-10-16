import { appTest } from "./app-fixture";
import { FPage } from "./pages/f-page";
import { GPage } from "./pages/g-page";

const localTest = appTest.extend({});

export const fTest = localTest.extend<{ f: FPage }>({
  f: async ({}, use) => {
    await use(new FPage());
  },
});

export const gTest = fTest.extend<{ g: GPage }>({
  g: async ({}, use) => {
    await use(new GPage());
  },
});
