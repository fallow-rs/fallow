import type { Spec } from "@fixture/native-spec";
import type { Output } from "@fixture/typings-output";
import "@fixture/no-entry";

export const run = (spec: Spec, output: Output): void => {
  spec.run();
  console.log(output.value);
};
