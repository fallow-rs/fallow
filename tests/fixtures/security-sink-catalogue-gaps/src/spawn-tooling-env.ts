// A release script runs a configured CLI path without a shell.
import { spawnSync } from "node:child_process";

const FLY = process.env.FLY_BIN ?? "flyctl";

export function deploy(): void {
  spawnSync(FLY, ["deploy"]);
}
