import { test as base } from "@playwright/test";
import { FixtureOrchestrator, type AppFixture } from "./fixture-orchestrator";

type PrimaryFixtures = {
  app: AppFixture;
};

export const testPrimary = base.extend<PrimaryFixtures>({
  app: async ({}, use) => {
    const orchestrator = new FixtureOrchestrator();

    await use(orchestrator.createApp());
  },
});
