import { test as base } from '@playwright/test';
import { SidebarActionsDirect } from '../src/pages/sidebar-actions-direct';

type MyFixtures = {
  appUi: {
    step: {
      sidebar: SidebarActionsDirect;
    };
  };
};

export function appTestDirectReturn() {
  return base.extend<MyFixtures>({
    appUi: async ({}, use) => {
      await use({
        step: {
          sidebar: new SidebarActionsDirect(),
        },
      });
    },
  });
}
