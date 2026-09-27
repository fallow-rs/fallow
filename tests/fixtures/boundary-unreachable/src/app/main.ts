import { exec } from "node:child_process";
import { readSecret } from "../secret/store";

export function run(): void {
  exec("echo reachable");
  console.log(readSecret());
}
