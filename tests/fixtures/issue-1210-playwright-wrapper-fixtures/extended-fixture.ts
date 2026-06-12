import { testPrimary } from "./primary-fixture";

type ExtendedFixtures = {
  variant: string;
};

export const extendedTest = testPrimary.extend<ExtendedFixtures>({
  variant: async ({}, use) => {
    await use("extended");
  },
});
