import { test } from './fixtures';

test('login', async ({ loginPage }) => {
  loginPage.fillForm();
});
