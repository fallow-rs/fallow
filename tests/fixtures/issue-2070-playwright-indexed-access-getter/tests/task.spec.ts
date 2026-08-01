import { test } from './fixtures/task-fixture';

test('finds a task', async ({ tasks }) => {
  await tasks.assert.hasTaskForReference('synthetic-reference-123');
});
