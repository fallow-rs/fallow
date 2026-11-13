// Tooling code runs a binary of this repository without a shell.
import { spawn } from "node:child_process";
import { resolve } from "node:path";

const INSPECTOR = resolve(import.meta.dirname, "../node_modules/.bin/inspector");

export function inspect(): void {
  spawn(INSPECTOR, ["--cli"], { stdio: "inherit" });
}
