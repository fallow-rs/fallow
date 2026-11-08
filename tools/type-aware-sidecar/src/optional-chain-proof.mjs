// Proves that an optional-chaining mutant is equivalent to the original code.
//
// The mutants `a?.b` -> `a.b`, `a?.()` -> `a()` and `a?.[k]` -> `a[k]` change
// nothing when the type of `a` excludes `null` and `undefined`. The proof
// trusts the checker, so it needs `strictNullChecks` (a type excludes nullish
// values only then) and `noUncheckedIndexedAccess` (otherwise `xs[0]` has a
// non-nullable type and is `undefined` for an empty array). When the checker
// cannot show the receiver safe, the proof says nothing and the mutant keeps
// its normal check.

import path from "node:path";

import { ast, projectCompilerOptions, sync } from "./backend.mjs";

const { TypeFlags } = sync;
const {
  isAsExpression,
  isCallExpression,
  isElementAccessExpression,
  isNonNullExpression,
  isParenthesizedExpression,
  isPropertyAccessExpression,
  isSatisfiesExpression,
  isTypeAssertion,
} = ast;

/** The reason that the response gives for a proven mutant. */
export const TYPE_PROVEN_REASON = "type-proven-never-nullish";

const OPTIONAL_TOKEN = "?.";
const TYPESCRIPT_EXTENSIONS = new Set([".ts", ".tsx", ".mts", ".cts"]);
const MAX_UNION_DEPTH = 8;
const NON_NULLABLE_FLAGS = TypeFlags.DefinitelyNonNullable;
const UNION_OR_INTERSECTION = TypeFlags.Union | TypeFlags.Intersection;

const strictNullChecks = (options) =>
  options.strictNullChecks === true ||
  (options.strictNullChecks === undefined && options.strict === true);

// Whether the compiler options make the type of a receiver a proof.
const allowsTypeProof = (project, absolutePath) => {
  const options = projectCompilerOptions(project);
  return (
    TYPESCRIPT_EXTENSIONS.has(path.extname(absolutePath).toLowerCase()) &&
    strictNullChecks(options) &&
    options.noUncheckedIndexedAccess === true
  );
};

const neverNullish = (type, depth) => {
  if (depth > MAX_UNION_DEPTH) return false;
  if ((type.flags & UNION_OR_INTERSECTION) !== 0) {
    const members = type.getTypes?.() ?? [];
    return members.length > 0 && members.every((member) => neverNullish(member, depth + 1));
  }
  return type.flags !== 0 && (type.flags & ~NON_NULLABLE_FLAGS) === 0;
};

// A cast or a non-null assertion states the type without proof, so the
// receiver of such a link is not trusted.
const trustedReceiver = (expression) => {
  let current = expression;
  while (isParenthesizedExpression(current) || isSatisfiesExpression(current)) {
    current = current.expression;
  }
  return !(isAsExpression(current) || isTypeAssertion(current) || isNonNullExpression(current))
    ? current
    : null;
};

const optionalLinkReceiver = (node) => {
  if (node.questionDotToken === undefined) return null;
  const hasReceiver =
    isPropertyAccessExpression(node) || isElementAccessExpression(node) || isCallExpression(node);
  return hasReceiver ? node.expression : null;
};

// Maps the UTF-16 end offset of each `?.` token to the receiver it follows.
const receiversByLinkEnd = (sourceFile) => {
  const receivers = new Map();
  const visit = (node) => {
    const receiver = optionalLinkReceiver(node);
    if (receiver !== null) receivers.set(node.questionDotToken.end, receiver);
    node.forEachChild((child) => {
      visit(child);
      return undefined;
    });
  };
  visit(sourceFile);
  return receivers;
};

const isOptionalLink = (original, mutant) =>
  original.subarray(mutant.start, mutant.end).toString("utf8") === OPTIONAL_TOKEN;

const utf16Offset = (original, byteOffset) =>
  original.subarray(0, byteOffset).toString("utf8").length;

const receiverIsNeverNullish = (project, receiver) => {
  const trusted = trustedReceiver(receiver);
  if (trusted === null) return false;
  try {
    const type = project.checker.getTypeAtLocation(trusted);
    return type !== undefined && neverNullish(type, 0);
  } catch {
    return false;
  }
};

/**
 * Returns the ids of the mutants of `file` that remove an optional link from
 * a receiver whose type is never nullish. `project` is the project of the
 * original file, before any mutant is applied.
 */
export const typeProvenEquivalents = (project, file, original) => {
  const links = file.mutants.filter((mutant) => isOptionalLink(original, mutant));
  if (links.length === 0 || !allowsTypeProof(project, file.absolutePath)) return new Set();
  const sourceFile = project.program.getSourceFile(file.absolutePath);
  if (!sourceFile) return new Set();
  try {
    const receivers = receiversByLinkEnd(sourceFile);
    const proven = links.filter((mutant) => {
      const receiver = receivers.get(utf16Offset(original, mutant.end));
      return receiver !== undefined && receiverIsNeverNullish(project, receiver);
    });
    return new Set(proven.map((mutant) => mutant.id));
  } catch {
    return new Set();
  }
};
