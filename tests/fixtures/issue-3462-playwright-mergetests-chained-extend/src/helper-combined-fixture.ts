import { mergeTests } from "@playwright/test";
import { appTest } from "./app-fixture";
import { AuditPage } from "./pages/audit-page";

type AuditFixtures = {
  audit: AuditPage;
};

export const auditTest = () =>
  mergeTests(appTest).extend<AuditFixtures>({
    audit: async ({}, use) => {
      await use(new AuditPage());
    },
  });
