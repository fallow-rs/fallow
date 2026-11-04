import { execSync, spawn, spawnSync } from "node:child_process";

export const ip = spawnSync("hyprctl", ["version"]);
export const status = execSync("tailscale status && git log");
export const lint = spawn("installed-tool", ["src"]);
