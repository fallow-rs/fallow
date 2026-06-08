import * as path from "node:path";
import type { FixAction } from "./types.js";

interface FixPreviewNavigateItem {
  readonly action: "navigate";
  readonly label: string;
  readonly description: string;
  readonly detail: string;
  readonly fix: FixAction;
}

interface FixPreviewApplyAllItem {
  readonly action: "apply-all";
  readonly label: string;
  readonly description: string;
}

type FixPreviewItem = FixPreviewNavigateItem | FixPreviewApplyAllItem;

export const buildFixArgs = (dryRun: boolean, production: boolean | undefined): string[] => {
  const args = dryRun
    ? ["fix", "--dry-run", "--format", "json", "--quiet"]
    : ["fix", "--yes", "--format", "json", "--quiet"];

  if (production) {
    args.push("--production");
  }

  return args;
};

const getFixLabel = (fix: FixAction): string => fix.name ?? fix.package ?? fix.file ?? "unknown";

const getFixDetail = (fix: FixAction): string =>
  fix.path ? `${fix.path}${fix.line ? `:${fix.line}` : ""}` : (fix.location ?? "");

export const createFixPreviewItems = (fixes: ReadonlyArray<FixAction>): FixPreviewItem[] => [
  ...fixes.map((fix) => ({
    label: getFixLabel(fix),
    description: fix.type.replace(/_/g, " "),
    detail: getFixDetail(fix),
    action: "navigate" as const,
    fix,
  })),
  {
    label: "Apply all fixes",
    description: `${fixes.length} fix${fixes.length === 1 ? "" : "es"}`,
    action: "apply-all" as const,
  },
];

export const resolveFixLocation = (
  root: string,
  fix: FixAction,
): { absolutePath: string; line: number } | null => {
  const filePath = fix.path ?? fix.file;
  if (!filePath) {
    return null;
  }

  return {
    absolutePath: path.isAbsolute(filePath) ? filePath : path.resolve(root, filePath),
    line: Math.max(0, (fix.line ?? 1) - 1),
  };
};
