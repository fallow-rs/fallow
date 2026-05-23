import { runTask } from "#nitro/runtime/task";
import { selfReference } from "nitro/self";

export { runTask } from "#nitro/runtime/task";

void import("#nitro/virtual/polyfills");
void import("#nitro/runtime/missing");
void import("#other/alias");

runTask();
selfReference();
