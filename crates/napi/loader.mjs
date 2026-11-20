// ES module entry of the package. Node cannot find named exports in
// loader.js because it assigns `module.exports` from the native binding at
// run time. This file lists each binding function as a named export. All
// exports come from the one CommonJS instance of loader.js.
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const binding = require("./loader.js");

export const {
  computeComplexity,
  computeHealth,
  detectArchitecture,
  detectBoundaryViolations,
  detectCircularDependencies,
  detectDeadCode,
  detectDuplication,
  detectFeatureFlags,
  detectSimilarCode,
} = binding;

export default binding;
