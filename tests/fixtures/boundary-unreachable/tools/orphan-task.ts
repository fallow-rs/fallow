import { exec } from "node:child_process";
import { readSecret } from "../src/secret/store";

exec("echo orphan");
console.log(readSecret());
