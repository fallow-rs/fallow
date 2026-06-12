import { mergedTest } from "./merged-fixture";

type ExtendedMergedFixtures = {
  flavor: string;
};

export const extendedMergedTest = mergedTest.extend<ExtendedMergedFixtures>({
  flavor: async ({}, use) => {
    await use("extended-merged");
  },
});
