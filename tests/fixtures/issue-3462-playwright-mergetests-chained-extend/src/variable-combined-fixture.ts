import { mergeTests } from "@playwright/test";
import { appTest } from "./app-fixture";
import { SettingsPage } from "./pages/settings-page";

type SettingsFixtures = {
  settings: SettingsPage;
};

const mergedTest = mergeTests(appTest);

export const variableTest = mergedTest.extend<SettingsFixtures>({
  settings: async ({}, use) => {
    await use(new SettingsPage());
  },
});
