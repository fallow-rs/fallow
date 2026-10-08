/**
 * Plain-language wording for findings. The analyzers speak in rule ids and
 * metric acronyms (`path-traversal`, MI, CRAP); a reader scanning a list
 * needs the consequence instead. Every label here is a short noun phrase
 * that reads well after a file name.
 */

import type { VizHealthFile } from "./types";

const SECURITY_LABELS: Record<string, string> = {
  "cleartext-transport": "Sends data unencrypted",
  "code-injection": "Runs code built from input",
  "command-injection": "Shell command built from input",
  "deprecated-cipher": "Deprecated cipher",
  "dynamic-module-load": "Loads a module chosen at runtime",
  "dynamic-regex": "Regex built from input",
  "electron-unsafe-webpreferences": "Unsafe Electron web preferences",
  "header-injection": "HTTP header built from input",
  "insecure-cookie": "Cookie without secure flags",
  "insecure-randomness": "Predictable randomness",
  "insecure-temp-file": "Insecure temp file",
  "jwt-alg-none": "JWT accepts no signature",
  "jwt-verify-missing-algorithms": "JWT verify without algorithm list",
  "llm-call-injection": "Prompt built from input",
  "mass-assignment": "Mass assignment from input",
  "mysql-multiple-statements": "Multiple SQL statements allowed",
  "nosql-injection": "NoSQL query built from input",
  "open-redirect": "Redirect target from input",
  "path-traversal": "File path built from input",
  "permissive-cors": "Permissive CORS",
  "postmessage-wildcard-origin": "postMessage to any origin",
  "prototype-pollution": "Prototype pollution",
  "redos-regex": "Regex open to ReDoS",
  "resource-amplification": "Unbounded resource use",
  "route-send-file": "Sends a file chosen by input",
  "secret-pii-log": "May log secrets or personal data",
  "sql-injection": "SQL built from input",
  ssrf: "Outbound request to a URL from input",
  "template-escape-bypass": "Template escaping bypassed",
  "tls-validation-disabled": "TLS validation disabled",
  "unsafe-buffer-alloc": "Unsafe buffer allocation",
  "unsafe-deserialization": "Unsafe deserialization",
  "weak-crypto": "Weak cryptography",
  "webview-injection": "Webview content from input",
  "world-writable-permission": "World-writable file permission",
  "xpath-injection": "XPath built from input",
  xxe: "XML external entities enabled",
  "zip-slip": "Archive path escapes target folder",
};

/** Human label for a security rule category, falling back to the id itself. */
export const securityCategoryLabel = (category: string | null, fallback: string): string => {
  if (!category) return fallback;
  const key = category.replace(/^security-/, "").replace(/-\d+$/, "");
  return SECURITY_LABELS[key] ?? key.replaceAll("-", " ");
};

/** Ordinal for a severity word: 3 = high or worse, 2 = medium, 1 = low. */
export const severityRank = (severity: string): number => {
  switch (severity.toLowerCase()) {
    case "critical":
    case "high":
    case "error":
      return 3;
    case "medium":
    case "moderate":
    case "warning":
    case "warn":
      return 2;
    default:
      return 1;
  }
};

/** Maintainability index bands, matching the common 0-100 MI reading. */
const maintainabilityPhrase = (mi: number): string | null => {
  if (mi < 50) return `low maintainability (${mi.toFixed(0)}/100)`;
  if (mi < 70) return `medium maintainability (${mi.toFixed(0)}/100)`;
  return null;
};

/**
 * One-line reason a file is in the Health list, largest risk first. CRAP
 * is complexity weighted by missing tests, so it is shown in those words
 * and not as an acronym.
 */
export const healthReason = (health: VizHealthFile | undefined, fallback: string): string => {
  if (!health) return fallback;
  const parts: string[] = [];
  if (health.crap_max >= 30) parts.push("high complexity, no test coverage");
  else if (health.crap_max >= 10) parts.push("high complexity, low test coverage");
  const mi = maintainabilityPhrase(health.maintainability_index);
  if (mi) parts.push(mi);
  if (health.fan_in >= 20) parts.push(`imported by ${health.fan_in} files`);
  if (parts.length === 0) return fallback;
  const sentence = parts.join(", ");
  return sentence.charAt(0).toUpperCase() + sentence.slice(1);
};

/**
 * Plain wording for how strongly a candidate links input to the call:
 * `arg-level` traces the argument itself, `module-level` only shows that
 * input reaches the file.
 */
export const confidenceLabel = (confidence: string): string => {
  switch (confidence) {
    case "arg-level":
      return "input traced to the call";
    case "module-level":
      return "input reaches the file only";
    default:
      return `${confidence} confidence`;
  }
};
