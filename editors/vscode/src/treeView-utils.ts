import * as path from "node:path";

export interface ResolvedPath {
  readonly absolute: string;
  readonly relative: string;
}

export const resolveFilePath = (
  filePath: string | undefined,
  workspaceRoot: string | undefined,
): ResolvedPath => {
  if (!filePath) {
    return { absolute: "", relative: "" };
  }
  const absolute =
    workspaceRoot && !path.isAbsolute(filePath) ? path.resolve(workspaceRoot, filePath) : filePath;
  const relative = workspaceRoot ? path.relative(workspaceRoot, absolute) : filePath;
  return { absolute, relative };
};
