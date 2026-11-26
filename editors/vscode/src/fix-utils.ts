import * as path from "node:path";
import type { FallowFixResult, FixAction } from "./types.js";
import { appendAllowRemoteExtends, type BuiltCliArgs } from "./cli-args-utils.js";

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

/**
 * Build the argument vector of a `fallow fix` run. `allowRemoteExtends`
 * forwards `--allow-remote-extends` when `cliVersion` knows the flag.
 */
export const buildFixArgs = (
  dryRun: boolean,
  production: boolean | undefined,
  allowRemoteExtends: boolean,
  cliVersion: string | null,
): BuiltCliArgs => {
  const args = dryRun
    ? ["fix", "--dry-run", "--format", "json", "--quiet"]
    : ["fix", "--yes", "--format", "json", "--quiet"];

  if (production) {
    args.push("--production");
  }

  const skipped = appendAllowRemoteExtends(args, allowRemoteExtends, cliVersion);
  return { args, skipped: skipped ? [skipped] : [] };
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

/**
 * The reason and the next step for an apply run that applied no fix but
 * skipped some. The CLI writes the next step into the `help` of each skipped
 * entry, so the most frequent help is the one that unblocks the most fixes.
 * Return `null` when the run applied a fix or skipped none with help.
 */
export const zeroAppliedAdvice = (result: FallowFixResult): string | null => {
  if (result.dry_run || result.total_fixed > 0) {
    return null;
  }
  const counts = new Map<string, number>();
  for (const fix of result.fixes) {
    if (fix.skipped && fix.help) {
      counts.set(fix.help, (counts.get(fix.help) ?? 0) + 1);
    }
  }
  const ranked = [...counts.entries()].toSorted((a, b) => b[1] - a[1]);
  const first = ranked[0];
  if (!first) {
    return null;
  }
  const others = ranked.length - 1;
  if (others === 0) {
    return first[0];
  }
  return `${first[0]} ${others} other reason${others === 1 ? "" : "s"} also held back fixes. Run \`fallow fix --dry-run\` in a terminal to see each one.`;
};
