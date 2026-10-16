import { test as base } from "@playwright/test";

export const appTest = base.extend<{ token: string }>({
  token: async ({}, use) => {
    await use("token");
  },
});
