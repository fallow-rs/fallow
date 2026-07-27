import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { spawnSync } from "node:child_process";

const hashJson = (value, suffix = "") =>
  createHash("sha256")
    .update(`${JSON.stringify(value)}${suffix}`)
    .digest("hex");

export const normalizedEvidenceResponseDigest = (response) => {
  const normalized = structuredClone(response);
  delete normalized.elapsed_ms;
  delete normalized.phase_timings_ms;
  return hashJson(normalized, "\n");
};

export const requestDigest = (request) => hashJson(request);

export const digestSet = (digests) =>
  createHash("sha256")
    .update(`${digests.toSorted().join("\n")}\n`)
    .digest("hex");

export const checkerEvidenceRequest = ({
  project,
  candidates,
  outDir,
  projectRoot,
  protocolVersion,
}) => {
  const root = projectRoot(project, outDir);
  return {
    root,
    request: {
      protocol_version: protocolVersion,
      operation: "semantic-queries",
      root,
      projects: [],
      evidence_limit: 40,
      queries: candidates.map((candidate, id) => ({
        id,
        operation: "symbol-use",
        symbol: {
          path: candidate.path,
          namespace: "value",
          declaration_kind: candidate.kind,
          exported_name: candidate.member_name,
          local_name: candidate.member_name,
          line: candidate.line,
          col: candidate.col,
          owner: candidate.parent_name,
        },
      })),
    },
  };
};

const sidecarInvocation = (sidecarBin) =>
  process.platform === "win32" && /\.(?:bat|cmd)$/iu.test(sidecarBin)
    ? {
        binary: process.env.ComSpec ?? "cmd.exe",
        args: ["/D", "/S", "/C", `"${sidecarBin}"`],
      }
    : { binary: sidecarBin, args: [] };

const persistSidecarOutput = ({ stdoutPath, stderrPath, stdout, stderr, maximumStderrBytes }) => {
  mkdirSync(dirname(stdoutPath), { recursive: true });
  writeFileSync(stdoutPath, stdout);
  writeFileSync(stderrPath, stderr.slice(0, maximumStderrBytes));
};

const requireSuccessfulSidecar = (result, fail) => {
  if (result.error) fail(`type-aware evidence sidecar failed: ${result.error.message}`);
  if (result.status === 0) return;
  const detail = String(result.stderr).trim().slice(0, 2_000);
  const outcome = result.status === null ? result.signal : result.status;
  fail(`type-aware evidence sidecar exited with ${outcome}: ${detail}`);
};

const parseSidecarResponse = (stdout, fail) => {
  try {
    return JSON.parse(stdout);
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    fail(`type-aware evidence sidecar emitted invalid JSON: ${detail}`);
  }
};

const validEnvelope = (response, protocolVersion, isObject) =>
  [
    isObject(response),
    response.protocol_version === protocolVersion,
    response.operation === "semantic-queries",
    typeof response.sidecar_version === "string",
    response.backend === "typescript-go",
    typeof response.backend_version === "string",
    Array.isArray(response.results),
  ].every(Boolean);

const validEvidenceLocation = (location, normalizedRelativePath, isObject, index) =>
  [
    isObject(location),
    normalizedRelativePath(location.path, `results evidence ${index} path`) === location.path,
    Number.isInteger(location.line),
    location.line >= 1,
    Number.isInteger(location.col),
    location.col >= 0,
  ].every(Boolean);

const validateEvidence = (evidence, dependencies) => {
  for (const [index, location] of evidence.entries()) {
    if (
      !validEvidenceLocation(
        location,
        dependencies.normalizedRelativePath,
        dependencies.isObject,
        index,
      )
    ) {
      dependencies.fail("type-aware evidence sidecar response has invalid source evidence");
    }
  }
};

const validateResult = (queryResult, expected, seen, dependencies) => {
  const valid = [
    dependencies.isObject(queryResult),
    expected.has(queryResult.query_id),
    expected.get(queryResult.query_id) === queryResult.operation,
    !seen.has(queryResult.query_id),
    dependencies.queryStatuses.has(queryResult.status),
    Array.isArray(queryResult.evidence),
  ].every(Boolean);
  if (!valid)
    dependencies.fail(
      "type-aware evidence sidecar response has invalid query identity or evidence",
    );
  validateEvidence(queryResult.evidence, dependencies);
  seen.add(queryResult.query_id);
};

const validateResponse = (response, request, dependencies) => {
  if (!validEnvelope(response, dependencies.protocolVersion, dependencies.isObject)) {
    dependencies.fail(
      "type-aware evidence sidecar response has invalid provenance or envelope fields",
    );
  }
  const expected = new Map(request.queries.map((query) => [query.id, query.operation]));
  const seen = new Set();
  for (const queryResult of response.results)
    validateResult(queryResult, expected, seen, dependencies);
  if (seen.size !== expected.size) {
    dependencies.fail("type-aware evidence sidecar response omitted one or more requested queries");
  }
};

export const runExactSidecarRequest = ({
  sidecarBin,
  request,
  stdoutPath,
  stderrPath,
  timeoutMs,
  maxResponseBytes,
  dependencies,
}) => {
  const invocation = sidecarInvocation(sidecarBin);
  const result = spawnSync(invocation.binary, invocation.args, {
    cwd: dirname(sidecarBin),
    env: dependencies.minimalEnvironment(null),
    input: `${JSON.stringify(request)}\n`,
    encoding: "utf8",
    maxBuffer: maxResponseBytes,
    timeout: timeoutMs,
    killSignal: "SIGKILL",
  });
  const stdout = result.stdout ?? "";
  const stderr = result.stderr ?? "";
  persistSidecarOutput({
    stdoutPath,
    stderrPath,
    stdout,
    stderr,
    maximumStderrBytes: dependencies.maximumStderrBytes,
  });
  requireSuccessfulSidecar(result, dependencies.fail);
  if (Buffer.byteLength(stdout) > maxResponseBytes) {
    dependencies.fail(`type-aware evidence sidecar exceeded ${maxResponseBytes} response bytes`);
  }
  const response = parseSidecarResponse(stdout, dependencies.fail);
  validateResponse(response, request, dependencies);
  return response;
};
