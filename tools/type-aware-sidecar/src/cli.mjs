import { analyzeClassMemberUses } from "./typescript-go.mjs";
import { analyzeSemanticQueries } from "./semantic.mjs";
import {
  createResponse,
  createSemanticResponse,
  createStatusResponse,
  parseRequest,
} from "./protocol.mjs";

const MAX_REQUEST_BYTES = 8 * 1024 * 1024;

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

export const run = async ({ input, output, args = [] }) => {
  if (args.length === 1 && args[0] === "--status") {
    output.write(`${JSON.stringify(createStatusResponse())}\n`);
    return;
  }
  if (args.length > 0) {
    throw new Error(`unknown argument: ${args[0]}`);
  }
  const startedAt = performance.now();
  const source = await readAll(input);
  let rawRequest;
  try {
    rawRequest = JSON.parse(source);
  } catch {
    throw new Error("stdin must contain one valid JSON request");
  }
  if (
    rawRequest?.protocol_version === 3 &&
    rawRequest?.operation === "status" &&
    Object.keys(rawRequest).length === 2
  ) {
    output.write(`${JSON.stringify(createStatusResponse())}\n`);
    return;
  }

  const request = parseRequest(rawRequest);
  const result =
    request.protocolVersion === 2
      ? analyzeClassMemberUses(request)
      : analyzeSemanticQueries(request);
  const responseFactory = request.protocolVersion === 2 ? createResponse : createSemanticResponse;
  const response = responseFactory({ ...result, elapsedMs: performance.now() - startedAt });
  output.write(`${JSON.stringify(response)}\n`);
};
