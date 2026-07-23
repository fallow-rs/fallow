import type { TypeAwareSettings } from "./config.js";

export type TypeAwareArgsOptions = TypeAwareSettings;

export const appendTypeAwareArgs = (
  args: string[],
  options: TypeAwareArgsOptions | undefined,
): void => {
  if (!options?.enabled) return;

  args.push("--type-aware");
  for (const project of options.projects) {
    args.push("--type-aware-project", project);
  }
  if (options.require !== "best-effort") {
    args.push("--type-aware-require", options.require);
  }
};
