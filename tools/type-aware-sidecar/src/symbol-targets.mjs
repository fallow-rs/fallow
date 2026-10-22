import { sync } from "./backend.mjs";

import {
  declarationName,
  declarationNamespaces,
  declarationsForSymbol,
  stableDeclarationKey,
} from "./semantic-identity.mjs";

const { SymbolFlags } = sync;

export const targetName = (declaration) => declarationName(declaration) ?? "default";

// A target key contains the declaration name. The name check skips the key
// and its file identity for every declaration that cannot match.
const targetsForDeclaration = (state, declaration, namespace) =>
  state.targetNames.has(targetName(declaration))
    ? (state.targetsByKey.get(stableDeclarationKey(declaration, namespace)) ?? [])
    : [];

export const matchingSymbolEntries = (project, symbol, namespace, state) => {
  const declarations = declarationsForSymbol(project, symbol);
  const matchingLane = declarations.filter((declaration) =>
    declarationNamespaces(declaration).has(namespace),
  );
  if (matchingLane.length > 0) {
    return new Set(
      matchingLane.flatMap((declaration) => targetsForDeclaration(state, declaration, namespace)),
    );
  }
  return new Set(
    declarations.flatMap((declaration) =>
      [...declarationNamespaces(declaration)].flatMap((declaredNamespace) =>
        targetsForDeclaration(state, declaration, declaredNamespace),
      ),
    ),
  );
};

const resolveAliasEntries = (project, symbol, namespace, state) => {
  const entries = new Set();
  const seen = new Set();
  let current = symbol;
  while (current && !seen.has(current)) {
    seen.add(current);
    matchingSymbolEntries(project, current, namespace, state).forEach((entry) =>
      entries.add(entry),
    );
    if ((current.flags & SymbolFlags.Alias) === 0) break;
    const aliased = project.checker.getAliasedSymbol(current);
    if (project.checker.isUnknownSymbol(aliased) || aliased === current) break;
    current = aliased;
  }
  return entries;
};

// The scan visits every identifier, so one symbol occurs many times. Each
// checker call is a round trip, and the targets do not change during a batch,
// so the batch keeps one answer for each symbol and namespace.
export const matchingAliasEntries = (project, symbol, namespace, state) => {
  if (!symbol) return new Set();
  const bySymbol = state.aliasEntriesByProject.get(project) ?? new Map();
  state.aliasEntriesByProject.set(project, bySymbol);
  const byNamespace = bySymbol.get(symbol) ?? new Map();
  bySymbol.set(symbol, byNamespace);
  if (!byNamespace.has(namespace)) {
    byNamespace.set(namespace, resolveAliasEntries(project, symbol, namespace, state));
  }
  return byNamespace.get(namespace);
};

export const recordSymbolUncertainty = (state, entry, reasonCode) => {
  state.uncertaintiesByQuery.get(entry.query.id).add(reasonCode);
};
