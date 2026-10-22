import { ast } from "./backend.mjs";
import { patternOfKey, patternSourceType } from "./shorthand-references.mjs";
import { matchingSymbolEntries, recordSymbolUncertainty } from "./symbol-targets.mjs";

const { isComputedPropertyName, isElementAccessExpression, isStringLiteralLikeNode } = ast;

const isComputedAccess = (node) =>
  isElementAccessExpression(node) &&
  node.argumentExpression !== undefined &&
  !isStringLiteralLikeNode(node.argumentExpression);

const isComputedKey = (node) =>
  isComputedPropertyName(node) && !isStringLiteralLikeNode(node.expression);

// `object[key]` and `const { [key]: value } = object` both read a member of
// `object` that the checker cannot name. A null type is an unknown receiver.
const computedReceiverType = (project, node) => {
  if (isComputedAccess(node)) return project.checker.getTypeAtLocation(node.expression);
  if (!isComputedKey(node)) return undefined;
  const pattern = patternOfKey(node.expression);
  if (!pattern) return undefined;
  return patternSourceType(project, pattern) ?? null;
};

/**
 * Records a `dynamic-member-access` uncertainty for the class members that a
 * computed member access or a computed pattern key can read.
 */
export const recordComputedMemberAccess = (state, project, entries, node) => {
  if (entries.size === 0 || state.projectsWithUnknownDynamicAccess.has(project)) return;
  const receiverType = computedReceiverType(project, node);
  if (receiverType === undefined) return;
  const receiverProperties = receiverType ? project.checker.getPropertiesOfType(receiverType) : [];
  if (receiverProperties.length === 0) {
    entries.forEach((entry) => recordSymbolUncertainty(state, entry, "dynamic-member-access"));
    state.projectsWithUnknownDynamicAccess.add(project);
    return;
  }
  const matched = new Set(
    receiverProperties
      .filter((property) => state.candidateNames.has(property.name))
      .flatMap((property) => [...matchingSymbolEntries(project, property, "value", state)]),
  );
  matched.forEach((entry) => recordSymbolUncertainty(state, entry, "dynamic-member-access"));
};
