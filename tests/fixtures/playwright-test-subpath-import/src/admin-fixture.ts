import { test as base } from "playwright/test";
import { AdminPage } from "./pages/admin-page";

export const adminTest = base.extend<{ admin: AdminPage }>({
  admin: async ({}, use) => {
    await use(new AdminPage());
  },
});
