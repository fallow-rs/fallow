const isReExportUse = (use) => use.role.includes("re-export");

// The client credits a use only from a reachable file that reads the binding.
// One item for each file lets a capped result still show such a file.
const recordFileUse = (evidence, use, limit) => {
  const index = evidence.findIndex((kept) => kept.path === use.path);
  if (index === -1) {
    if (evidence.length < limit) evidence.push(use);
    return;
  }
  if (isReExportUse(evidence[index]) && !isReExportUse(use)) evidence[index] = use;
};

/**
 * Adds one reference to the bounded evidence of a query.
 * A `symbol-use` query keeps one item for each file. Other queries keep the first items.
 */
export const recordBoundedUse = (evidence, use, operation, limit) => {
  if (operation === "symbol-use") {
    recordFileUse(evidence, use, limit);
  } else if (evidence.length < limit) {
    evidence.push(use);
  }
};
