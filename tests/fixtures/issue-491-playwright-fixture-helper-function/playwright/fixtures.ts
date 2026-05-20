import { test as base } from '@playwright/test';
import { AdminActions } from '../src/pages/admin-actions';
import { LoginActions } from '../src/pages/login-actions';

type MyFixtures = {
  appUi: {
    step: {
      login: LoginActions;
      admin: AdminActions;
    };
  };
};

export function appTest() {
  return base.extend<MyFixtures>({
    appUi: async ({}, use) => {
      await use({
        step: {
          login: new LoginActions(),
          admin: new AdminActions(),
        },
      });
    },
  });
}
