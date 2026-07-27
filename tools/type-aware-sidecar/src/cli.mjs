import path from "node:path";
import { createInterface } from "node:readline";

import {
  ANALYSIS_OPERATION,
  SESSION_ENVELOPE_TYPES,
  STATUS_OPERATION,
  WIRE_PROTOCOL_VERSION,
} from "./generated-protocol.mjs";
import { analyzeSemanticQueries, createSemanticSession } from "./semantic.mjs";
import { createSemanticResponse, createStatusResponse, parseRequest } from "./protocol.mjs";

const MAX_REQUEST_BYTES = 8 * 1024 * 1024;
const [ANALYZE_ENVELOPE, SHUTDOWN_ENVELOPE] = SESSION_ENVELOPE_TYPES;
const STATUS_FIELDS = [
  ["protocol_version", WIRE_PROTOCOL_VERSION],
  ["operation", STATUS_OPERATION],
];

export const readAll = async (input, maximumBytes = MAX_REQUEST_BYTES) => {
  const chunks = [];
  let byteLength = 0;
  for await (const chunk of input) {
    const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    byteLength += buffer.byteLength;
    if (byteLength > maximumBytes) {
      throw new Error(`stdin exceeded the ${maximumBytes} byte request limit`);
    }
    chunks.push(buffer);
  }
  return Buffer.concat(chunks).toString("utf8");
};

const writeJson = (output, value) => {
  output.write(`${JSON.stringify(value)}\n`);
};

const handleArguments = (args, output) => {
  if (args.length === 1 && args[0] === "--status") {
    writeJson(output, createStatusResponse());
    return true;
  }
  if (args.length === 1 && args[0] === "--session") {
    return false;
  }
  if (args.length > 0) {
    throw new Error(`unknown argument: ${args[0]}`);
  }
  return false;
};

const sessionFileChanges = (value, root) => {
  if (value === undefined) return undefined;
  if (value?.invalidate_all === true && Object.keys(value).length === 1) {
    return { invalidateAll: true };
  }
  const allowed = new Set(["changed", "created", "deleted"]);
  if (
    typeof value !== "object" ||
    value === null ||
    Array.isArray(value) ||
    Object.keys(value).some((key) => !allowed.has(key))
  ) {
    throw new Error("session file_changes contains an unsupported field");
  }
  const resolveFiles = (files, field) => {
    if (files === undefined) return undefined;
    if (!Array.isArray(files)) throw new Error(`session file_changes.${field} must be an array`);
    return files.map((file) => {
      if (typeof file !== "string" || file.length === 0 || path.isAbsolute(file)) {
        throw new Error(`session file_changes.${field} must contain project-relative paths`);
      }
      const resolved = path.resolve(root, file);
      const relative = path.relative(root, resolved);
      if (relative === ".." || relative.startsWith(`..${path.sep}`)) {
        throw new Error(`session file_changes.${field} must stay within the session root`);
      }
      return resolved;
    });
  };
  return {
    ...(value.changed ? { changed: resolveFiles(value.changed, "changed") } : {}),
    ...(value.created ? { created: resolveFiles(value.created, "created") } : {}),
    ...(value.deleted ? { deleted: resolveFiles(value.deleted, "deleted") } : {}),
  };
};

const runSession = async (input, output) => {
  const lines = createInterface({ input, crlfDelay: Infinity });
  let session;
  let root;
  try {
    for await (const line of lines) {
      if (Buffer.byteLength(line, "utf8") > MAX_REQUEST_BYTES) {
        throw new Error(`session request exceeded the ${MAX_REQUEST_BYTES} byte limit`);
      }
      const envelope = parseJsonRequest(line);
      if (envelope?.type === SHUTDOWN_ENVELOPE) return;
      if (
        envelope?.type !== ANALYZE_ENVELOPE ||
        !Number.isSafeInteger(envelope.request_id) ||
        envelope.request_id < 0 ||
        !Number.isSafeInteger(envelope.revision) ||
        envelope.revision < 1
      ) {
        throw new Error("invalid semantic session envelope");
      }
      const request = parseRequest(envelope.request);
      if (!session) {
        root = request.root;
        session = createSemanticSession(root);
      }
      if (request.root !== root) throw new Error("semantic session root mismatch");
      const startedAt = performance.now();
      const result = session.analyze(request, {
        revision: envelope.revision,
        fileChanges: sessionFileChanges(envelope.file_changes, root),
      });
      writeJson(output, {
        request_id: envelope.request_id,
        revision: envelope.revision,
        response: responseFor(request, result, performance.now() - startedAt),
      });
    }
  } finally {
    session?.close();
  }
};

const parseJsonRequest = (source) => {
  try {
    return JSON.parse(source);
  } catch {
    throw new Error("stdin must contain one valid JSON request");
  }
};

const isStatusRequest = (request) =>
  Object.keys(request ?? {}).length === STATUS_FIELDS.length &&
  STATUS_FIELDS.every(([name, value]) => request[name] === value);

const responseFor = (request, result, elapsedMs) => {
  if (request.protocolVersion !== WIRE_PROTOCOL_VERSION) {
    throw new Error(`unsupported protocol_version ${String(request.protocolVersion)}`);
  }
  return createSemanticResponse({ ...result, elapsedMs });
};

export const run = async ({ input, output, args = [] }) => {
  if (args.length === 1 && args[0] === "--session") {
    await runSession(input, output);
    return;
  }
  if (handleArguments(args, output)) return;
  const startedAt = performance.now();
  const rawRequest = parseJsonRequest(await readAll(input));
  if (isStatusRequest(rawRequest)) {
    writeJson(output, createStatusResponse());
    return;
  }
  const request = parseRequest(rawRequest);
  if (rawRequest.operation !== ANALYSIS_OPERATION) {
    throw new Error(`unsupported operation ${String(rawRequest.operation)}`);
  }
  const result = analyzeSemanticQueries(request);
  writeJson(output, responseFor(request, result, performance.now() - startedAt));
};
