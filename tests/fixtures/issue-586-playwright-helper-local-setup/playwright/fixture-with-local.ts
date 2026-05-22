import { test as base } from '@playwright/test';
import { SidebarActionsLocal } from '../src/pages/sidebar-actions-local';

type MyFixtures = {
  appUi: {
    step: {
      sidebar: SidebarActionsLocal;
    };
  };
};

type UserRole = 'assistant' | 'anonymous';

export function appTestWithLocal(role: UserRole = 'assistant') {
  const storageState = role === 'assistant' ? 'assistant-auth.json' : undefined;

  return base.extend<MyFixtures>({
    storageState: async ({}, use) => {
      await use(storageState);
    },
    appUi: async ({}, use) => {
      await use({
        step: {
          sidebar: new SidebarActionsLocal(),
        },
      });
    },
  });
}
