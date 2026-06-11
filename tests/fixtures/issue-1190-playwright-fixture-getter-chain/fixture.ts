import { test as base } from "@playwright/test";
import { FixtureOrchestrator, type AppFixture } from "./fixture-orchestrator";

type MyFixtures = {
  app: AppFixture;
};

export const test = base.extend<MyFixtures>({
  app: async ({}, use) => {
    const orchestrator = new FixtureOrchestrator();

    await use(orchestrator.createApp());
  },
});
