import { defineConfig } from "oxlint";
import reactStrict from "lint-preset/react-strict";

export default defineConfig({ extends: [reactStrict] });
