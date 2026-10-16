import { mergeTests } from "@playwright/test";
import { appTest } from "./app-fixture";

export const test = mergeTests(appTest).extend({});
