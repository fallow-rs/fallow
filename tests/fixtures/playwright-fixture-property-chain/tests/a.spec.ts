import { test, expect } from '../fixtures/base';

test('details', async ({ app }) => {
  app.goto();
  expect(app.details.getParameter('x')).toBe('x');
  expect(app.details.input.select()).toBe(1);
});
