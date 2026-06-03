import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { runTests } from "@vscode/test-electron";

const extensionDevelopmentPath = path.resolve(__dirname, "../../..");
const extensionTestsPath = path.resolve(__dirname, "suite/index.js");
const vscodeTestCachePath = path.join(os.tmpdir(), "fallow-vscode-test-cache");
const fixtureWorkspacePath = path.resolve(
  extensionDevelopmentPath,
  "test/integration/fixtures/workspace/package.json"
);

const writeExecutable = (filePath: string, contents: string): void => {
  fs.writeFileSync(filePath, contents, "utf8");
  fs.chmodSync(filePath, 0o755);
};

const createFakeLsp = (binDir: string): string => {
  const lspPath = path.join(binDir, "fallow-lsp");
  writeExecutable(
    lspPath,
    `#!/usr/bin/env node
process.stdin.setEncoding("utf8");
let buffer = "";
const send = (message) => {
  const payload = JSON.stringify(message);
  process.stdout.write(\`Content-Length: \${Buffer.byteLength(payload, "utf8")}\\r\\n\\r\\n\${payload}\`);
};
const handle = (message) => {
  if (message.method === "initialize") {
    send({ jsonrpc: "2.0", id: message.id, result: { capabilities: {} } });
    return;
  }
  if (message.method === "shutdown") {
    send({ jsonrpc: "2.0", id: message.id, result: null });
    return;
  }
  if (message.id !== undefined) {
    send({
      jsonrpc: "2.0",
      id: message.id,
      error: { code: -32601, message: "Method not found" },
    });
    return;
  }
  if (message.method === "exit") {
    process.exit(0);
  }
};
process.stdin.on("data", (chunk) => {
  buffer += chunk;
  while (true) {
    const headerEnd = buffer.indexOf("\\r\\n\\r\\n");
    if (headerEnd === -1) {
      return;
    }
    const header = buffer.slice(0, headerEnd);
    const match = header.match(/Content-Length: (\\d+)/i);
    if (!match) {
      process.exit(1);
    }
    const contentLength = Number(match[1]);
    const messageStart = headerEnd + 4;
    if (buffer.length < messageStart + contentLength) {
      return;
    }
    const payload = buffer.slice(messageStart, messageStart + contentLength);
    buffer = buffer.slice(messageStart + contentLength);
    handle(JSON.parse(payload));
  }
});
`
  );
  return lspPath;
};

const createFakeCli = (binDir: string): void => {
  const cliPath = path.join(binDir, "fallow");
  const logPath = path.join(path.dirname(binDir), ".fallow-cli-log.jsonl");

  writeExecutable(
    cliPath,
    `#!/usr/bin/env node
const fs = require("node:fs");
const args = process.argv.slice(2);

// Respond to the extension's up-front version probe like a real binary: print
// a version and exit WITHOUT logging, so it does not pollute the analysis-call
// log the assertions read. Reported version is current, so version-gated flags
// are not skipped in these fixtures.
if (args[0] === "--version" || args[0] === "-V") {
  process.stdout.write("fallow 2.88.0\\n");
  process.exit(0);
}

const command = args[0] && !args[0].startsWith("-") ? args[0] : "combined";
const logPath = ${JSON.stringify(logPath)};
fs.appendFileSync(logPath, JSON.stringify({ command, args }) + "\\n");

const outputs = {
  "combined": {
    schema_version: 3,
    version: "2.45.0",
    elapsed_ms: 12,
    check: {
      unused_files: [{ path: "src/orphan.ts" }],
      unused_exports: [{ path: "src/index.ts", export_name: "unusedExport", line: 3, col: 1 }],
      unused_types: [],
      unused_dependencies: [],
      unused_dev_dependencies: [],
      unused_optional_dependencies: [{ path: "package.json", package_name: "fsevents" }],
      unused_enum_members: [],
      unused_class_members: [],
      unresolved_imports: [],
      unlisted_dependencies: [],
      duplicate_exports: [],
      type_only_dependencies: [],
      circular_dependencies: [],
    },
    dupes: {
      clone_groups: [{
        instances: [{
          file: "src/index.ts",
          start_line: 1,
          end_line: 3,
          start_col: 1,
          end_col: 1,
          fragment: "const value = 1;",
        }],
        token_count: 8,
        line_count: 3,
      }],
      clone_families: [],
      stats: {
        total_files: 1,
        files_with_clones: 1,
        total_lines: 3,
        duplicated_lines: 3,
        total_tokens: 8,
        duplicated_tokens: 8,
        clone_groups: 1,
        clone_instances: 1,
        duplication_percentage: 100,
      },
    },
  },
  "dead-code": {
    unused_files: [{ path: "src/orphan.ts" }],
    unused_exports: [{ path: "src/index.ts", export_name: "unusedExport", line: 3, col: 1 }],
    unused_types: [],
    unused_dependencies: [],
    unused_dev_dependencies: [],
    unused_optional_dependencies: [{ path: "package.json", package_name: "fsevents" }],
    unused_enum_members: [],
    unused_class_members: [],
    unresolved_imports: [],
    unlisted_dependencies: [],
    duplicate_exports: [],
    type_only_dependencies: [],
    circular_dependencies: [],
  },
  "dupes": {
    clone_groups: [{
      instances: [{
        file: "src/index.ts",
        start_line: 1,
        end_line: 3,
        start_col: 1,
        end_col: 1,
        fragment: "const value = 1;",
      }],
      token_count: 8,
      line_count: 3,
    }],
    clone_families: [],
    stats: {
      total_files: 1,
      files_with_clones: 1,
      total_lines: 3,
      duplicated_lines: 3,
      total_tokens: 8,
      duplicated_tokens: 8,
      clone_groups: 1,
      clone_instances: 1,
      duplication_percentage: 100,
    },
  },
  "fix": args.includes("--dry-run")
    ? {
        dry_run: true,
        fixes: [
          {
            type: "remove_export",
            name: "duplicateName",
            path: "src/first.ts",
            line: 2,
          },
          {
            type: "remove_export",
            name: "duplicateName",
            path: "src/second.ts",
            line: 7,
          },
        ],
        total_fixed: 2,
      }
    : {
        dry_run: false,
        fixes: [{
          type: "remove_export",
          name: "unusedExport",
          path: "src/index.ts",
          line: 3,
        }],
        total_fixed: 1,
      },
};

const output = outputs[command];
if (!output) {
  console.error("Unsupported fake fallow command:", command);
  process.exit(2);
}

process.stdout.write(JSON.stringify(output));
`
  );
};

const createWorkspace = (): string => {
  const workspaceDir = fs.mkdtempSync("/tmp/fv-");
  const vscodeDir = path.join(workspaceDir, ".vscode");
  const binDir = path.join(workspaceDir, "bin");
  const srcDir = path.join(workspaceDir, "src");

  fs.mkdirSync(vscodeDir, { recursive: true });
  fs.mkdirSync(binDir, { recursive: true });
  fs.mkdirSync(srcDir, { recursive: true });
  fs.copyFileSync(
    fixtureWorkspacePath,
    path.join(workspaceDir, "package.json")
  );
  fs.writeFileSync(path.join(srcDir, "index.ts"), "export const unusedExport = 1;\n");
  fs.writeFileSync(path.join(srcDir, "first.ts"), "export const duplicateName = 1;\n");
  fs.writeFileSync(
    path.join(srcDir, "second.ts"),
    "\n\n\n\n\n\nexport const duplicateName = 2;\n"
  );
  fs.writeFileSync(path.join(srcDir, "orphan.ts"), "export const orphan = true;\n");

  const lspPath = createFakeLsp(binDir);
  createFakeCli(binDir);

  fs.writeFileSync(
    path.join(vscodeDir, "settings.json"),
    JSON.stringify(
      {
        "fallow.autoDownload": false,
        "fallow.lspPath": lspPath,
      },
      null,
      2
    ),
    "utf8"
  );

  return workspaceDir;
};

const main = async (): Promise<void> => {
  const workspaceDir = createWorkspace();
  const extensionsDir = path.join(workspaceDir, ".vscode-test", "extensions");
  const userDataDir = path.join(workspaceDir, ".vscode-test", "user-data");

  try {
    await runTests({
      cachePath: vscodeTestCachePath,
      extensionDevelopmentPath,
      extensionTestsPath,
      launchArgs: [
        workspaceDir,
        "--disable-extensions",
        `--extensions-dir=${extensionsDir}`,
        `--user-data-dir=${userDataDir}`,
      ],
    });
  } catch (error) {
    console.error("Failed to run extension tests");
    throw error;
  } finally {
    fs.rmSync(workspaceDir, { recursive: true, force: true });
  }
};

void main();
