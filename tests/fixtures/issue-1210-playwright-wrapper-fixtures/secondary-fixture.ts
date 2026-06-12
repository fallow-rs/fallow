import { test as base } from "@playwright/test";

type SecondaryFixtures = {
  recordId: string;
};

export const testSecondary = base.extend<SecondaryFixtures>({
  recordId: async ({}, use) => {
    await use("ID-123");
  },
});
