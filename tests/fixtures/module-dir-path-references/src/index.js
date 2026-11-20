import { spawn } from "node:child_process";
import * as NodePath from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const __dirname = NodePath.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = NodePath.resolve(__dirname, "..");

export const agentPath = NodePath.join(__dirname, "../scripts/agent.mjs");

export const loadGenerator = () =>
  import(pathToFileURL(NodePath.join(__dirname, "..", "scripts", "generate.mjs")).href);

export function copyFixtures(path) {
  const fixtureDir = path.join(import.meta.dirname, "../scripts/fixtures");
  return path.join(fixtureDir, "peer.mjs");
}

export const runBehavior = (out) =>
  spawn(process.execPath, ["scripts/run.mjs", "--out", out], { cwd: REPO_ROOT });

export const libDir = NodePath.join(__dirname, "lib");
export const missing = NodePath.join(__dirname, "../dist/built.mjs");
