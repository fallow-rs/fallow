import { analyzeClassMemberUses } from "./typescript-go.mjs";
import { analyzeSemanticQueries } from "./semantic.mjs";
import {
  createResponse,
  createSemanticResponse,
  createStatusResponse,
  parseRequest,
} from "./protocol.mjs";

const MAX_REQUEST_BYTES = 8 * 1024 * 1024;
const STATUS_FIELDS = [
  ["protocol_version", 5],
  ["operation", "status"],
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
  if (args.length > 0) {
    throw new Error(`unknown argument: ${args[0]}`);
  }
  return false;
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

const analyze = (request) =>
  request.protocolVersion === 2 ? analyzeClassMemberUses(request) : analyzeSemanticQueries(request);

const responseFor = (request, result, elapsedMs) => {
  const responseFactory = request.protocolVersion === 2 ? createResponse : createSemanticResponse;
  return responseFactory({ ...result, elapsedMs });
};

export const run = async ({ input, output, args = [] }) => {
  if (handleArguments(args, output)) return;
  const startedAt = performance.now();
  const rawRequest = parseJsonRequest(await readAll(input));
  if (isStatusRequest(rawRequest)) {
    writeJson(output, createStatusResponse());
    return;
  }
  const request = parseRequest(rawRequest);
  const result = analyze(request);
  writeJson(output, responseFor(request, result, performance.now() - startedAt));
};
