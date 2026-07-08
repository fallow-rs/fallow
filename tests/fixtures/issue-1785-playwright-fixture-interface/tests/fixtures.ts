import { test as base } from '@playwright/test';
import { LoginPage } from '../src/pom';

interface MyFixtures {
  loginPage: LoginPage;
}

export const test = base.extend<MyFixtures>({
  loginPage: async ({ page }, use) => {
    await use(new LoginPage());
  },
});
