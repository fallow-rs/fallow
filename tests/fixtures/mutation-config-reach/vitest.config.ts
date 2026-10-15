import { defineConfig } from "vitest/config";
import { shared } from "./config/shared";

export default defineConfig({ test: { ...shared } });
