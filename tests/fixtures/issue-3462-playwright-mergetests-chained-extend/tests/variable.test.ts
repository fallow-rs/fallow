import { variableTest } from "../src/variable-combined-fixture";

variableTest("saves settings", async ({ settings }) => {
  settings.saveSettings();
});
