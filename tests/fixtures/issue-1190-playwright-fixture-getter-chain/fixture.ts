import { test as base } from "@playwright/test";
import { AsserterFactory } from "./asserter-factory";
import { MessageChecks } from "./message-checks";

type MyFixtures = {
  app: {
    assert: {
      messageChecks: MessageChecks;
    };
  };
};

export const test = base.extend<MyFixtures>({
  app: async ({}, use) => {
    const asserterFactory = new AsserterFactory();

    await use({
      assert: {
        messageChecks: asserterFactory.messageChecks,
      },
    });
  },
});
