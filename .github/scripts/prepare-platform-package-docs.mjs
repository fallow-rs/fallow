#!/usr/bin/env node
// Writes the README of every platform npm package and, with --license, copies
// the repository LICENSE next to it. npm always packs README and LICENSE files,
// so the platform tarballs carry both without a `files` entry.
//
// Usage:
//   node .github/scripts/prepare-platform-package-docs.mjs            write READMEs
//   node .github/scripts/prepare-platform-package-docs.mjs --license  also copy LICENSE
//   node .github/scripts/prepare-platform-package-docs.mjs --check    fail on README drift
//
// The release runs --license after `napi create-npm-dirs`, so the generated
// crates/napi/npm/* packages get the same files as the tracked npm/* packages.

import { copyFileSync, existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(fileURLToPath(new URL("../..", import.meta.url)));
const SCOPE = "@fallow-cli/";
const DOCS_URL = "https://fallow.tools/docs/";
const REPO_URL = "https://github.com/fallow-rs/fallow";

const OS_LABELS = { darwin: "macOS", linux: "Linux", win32: "Windows" };
const CPU_LABELS = { arm64: "ARM64", x64: "x64" };
const LIBC_LABELS = { glibc: "glibc", musl: "musl" };

/** Human label of the platform that a package manifest targets. */
export const platformLabel = (pkg) => {
  const os = pkg.os?.[0];
  const cpu = pkg.cpu?.[0];
  const osLabel = OS_LABELS[os];
  const cpuLabel = CPU_LABELS[cpu];
  if (!osLabel || !cpuLabel) {
    throw new Error(
      `${pkg.name}: unsupported os/cpu ${JSON.stringify({ os: pkg.os, cpu: pkg.cpu })}`,
    );
  }
  if (os === "darwin") {
    return `${osLabel} ${cpuLabel} (${cpu === "arm64" ? "Apple Silicon" : "Intel"})`;
  }
  // The CLI platform manifests have no `libc` field, so the name suffix decides.
  const libc =
    pkg.libc?.[0] ??
    (pkg.name.endsWith("-musl") ? "musl" : pkg.name.endsWith("-gnu") ? "glibc" : undefined);
  if (os === "linux" && libc) {
    return `${osLabel} ${cpuLabel} (${LIBC_LABELS[libc] ?? libc})`;
  }
  return `${osLabel} ${cpuLabel}`;
};

/** Package group: the CLI binaries, the similar-code sidecar, or the Node.js addon. */
export const packageGroup = (name) => {
  if (!name.startsWith(SCOPE)) {
    return null;
  }
  const rest = name.slice(SCOPE.length);
  if (rest.startsWith("fallow-similar-code-")) {
    return "similar-code";
  }
  if (rest.startsWith("fallow-node-")) {
    return "napi";
  }
  if (rest === "fallow-node" || rest.startsWith("fallow-")) {
    return null;
  }
  return "cli";
};

const GROUPS = {
  cli: {
    what: (label) =>
      `the ${label} binaries of the [\`fallow\`](https://www.npmjs.com/package/fallow) CLI`,
    parent: "fallow",
    install: "npm install --save-dev fallow",
    verification:
      "Before the first run, the `fallow` package checks the Ed25519 signature (`fallow.sig`) and the SHA-256 digest (`fallowDigests` in `package.json`) of the binary. When a check fails, `fallow` does not run the binary.",
  },
  "similar-code": {
    what: (label) =>
      `the ${label} binary of [\`fallow-similar-code\`](https://www.npmjs.com/package/fallow-similar-code), the local embedding sidecar for fallow similar-code analysis`,
    parent: "fallow-similar-code",
    install: "npm install --save-dev fallow fallow-similar-code",
    verification:
      "Before the first run, `fallow-similar-code` checks the Ed25519 signature (`fallow-similar-code.sig`) and the SHA-256 digest (`fallowDigests` in `package.json`) of the binary. When a check fails, it does not run the binary.",
  },
  napi: {
    what: (label) =>
      `the ${label} native addon of [\`@fallow-cli/fallow-node\`](https://www.npmjs.com/package/@fallow-cli/fallow-node), the Node.js bindings for fallow`,
    parent: "@fallow-cli/fallow-node",
    install: "npm install @fallow-cli/fallow-node",
    verification: null,
  },
};

/** README text of one platform package. */
export const renderReadme = (pkg) => {
  const group = GROUPS[packageGroup(pkg.name)];
  if (!group) {
    throw new Error(`${pkg.name}: not a platform package`);
  }
  const label = platformLabel(pkg);
  const lines = [
    `# ${pkg.name}`,
    "",
    `This package contains ${group.what(label)}. Fallow is codebase intelligence for TypeScript and JavaScript: unused code, duplication, circular dependencies, complexity and architecture checks.`,
    "",
    "## Install",
    "",
    `Do not install this package directly. Install \`${group.parent}\`. Your package manager then installs the platform package for your system from the \`optionalDependencies\` of \`${group.parent}\`.`,
    "",
    "```bash",
    group.install,
    "```",
    "",
  ];
  if (group.verification) {
    lines.push("## Verification", "", group.verification, "");
  }
  lines.push(
    "## Links",
    "",
    `- Documentation: ${DOCS_URL}`,
    `- Source code: ${REPO_URL}`,
    `- Security policy: ${REPO_URL}/blob/main/SECURITY.md`,
    "",
    "## License",
    "",
    "MIT. See `LICENSE`.",
    "",
  );
  return lines.join("\n");
};

const readManifest = (dir) => JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));

const listDirs = (parent) =>
  existsSync(parent)
    ? readdirSync(parent, { withFileTypes: true })
        .filter(
          (entry) => entry.isDirectory() && existsSync(join(parent, entry.name, "package.json")),
        )
        .map((entry) => join(parent, entry.name))
    : [];

/** Every platform package directory under `root`, tracked and generated. */
export const platformPackageDirs = (root = ROOT) =>
  [...listDirs(join(root, "npm")), ...listDirs(join(root, "crates/napi/npm"))]
    .filter((dir) => packageGroup(readManifest(dir).name) !== null)
    .toSorted();

/** Writes or checks the README (and optionally the LICENSE) of each platform package. */
export const preparePlatformPackageDocs = ({
  root = ROOT,
  check = false,
  license = false,
} = {}) => {
  const dirs = platformPackageDirs(root);
  const drift = [];
  for (const dir of dirs) {
    const readmePath = join(dir, "README.md");
    const expected = renderReadme(readManifest(dir));
    if (check) {
      const actual = existsSync(readmePath) ? readFileSync(readmePath, "utf8") : null;
      if (actual !== expected) {
        drift.push(readmePath);
      }
      continue;
    }
    writeFileSync(readmePath, expected);
    if (license) {
      copyFileSync(join(root, "LICENSE"), join(dir, "LICENSE"));
    }
  }
  if (license && !check) {
    const napiRoot = join(root, "crates/napi");
    if (existsSync(join(napiRoot, "package.json"))) {
      copyFileSync(join(root, "LICENSE"), join(napiRoot, "LICENSE"));
    }
  }
  return { dirs, drift };
};

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const args = new Set(process.argv.slice(2));
  const check = args.has("--check");
  const { dirs, drift } = preparePlatformPackageDocs({ check, license: args.has("--license") });
  if (dirs.length === 0) {
    console.error("no platform packages found");
    process.exit(1);
  }
  if (drift.length > 0) {
    console.error(
      `README drift in ${drift.length} platform package(s). Run node .github/scripts/prepare-platform-package-docs.mjs:\n  ${drift.join("\n  ")}`,
    );
    process.exit(1);
  }
  console.log(`${check ? "checked" : "prepared"} ${dirs.length} platform package(s)`);
}
