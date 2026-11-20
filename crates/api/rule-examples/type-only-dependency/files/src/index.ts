import type { ZodIssue } from "zod";

export const describeIssue = (issue: ZodIssue): string =>
  `${issue.path.join(".")}: ${issue.message}`;
