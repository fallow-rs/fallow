import { test as base } from '@playwright/test';
import { TaskAsserterFactory } from './task-asserter-factory';

type Fixtures = {
  tasks: {
    assert: TaskAsserterFactory['taskAsserter'];
  };
};

export const test = base.extend<Fixtures>({
  tasks: async ({}, use) => {
    const factory = new TaskAsserterFactory();
    await use({ assert: factory.taskAsserter });
  },
});
