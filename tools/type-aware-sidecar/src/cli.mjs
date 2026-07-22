import { analyzeClassMemberUses } from "./typescript-go.mjs";
import { createResponse, parseRequest } from "./protocol.mjs";

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

export const run = async ({ input, output }) => {
  const startedAt = performance.now();
  const source = await readAll(input);
  let rawRequest;
  try {
    rawRequest = JSON.parse(source);
  } catch {
    throw new Error("stdin must contain one valid JSON request");
  }

  const request = parseRequest(rawRequest);
  const result = analyzeClassMemberUses(request);
  const response = createResponse({
    ...result,
    elapsedMs: performance.now() - startedAt,
  });
  output.write(`${JSON.stringify(response)}\n`);
};
