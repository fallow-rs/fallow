// Generated from tools/mutation-runner/src/protocol.mjs. Do not edit.
import { ERROR_CODES, REQUEST_TYPES, WIRE_PROTOCOL_VERSION } from "./generated-protocol.mjs";

/** An error that the companion reports as a protocol `error` response. */
export class CompanionError extends Error {
  constructor(code, message, { retryable = false } = {}) {
    super(message);
    if (!ERROR_CODES.includes(code)) {
      throw new TypeError(`unknown companion error code: ${code}`);
    }
    this.code = code;
    this.retryable = retryable;
  }
}

/** Builds a response line object that copies the request id. */
export const response = (id, type, body = {}) => ({ v: WIRE_PROTOCOL_VERSION, id, type, ...body });

const errorClass = (error) =>
  error instanceof CompanionError
    ? { code: error.code, retryable: error.retryable }
    : { code: "internal", retryable: true };

/** Builds an `error` response. An unknown error is `internal` and retryable. */
export const errorResponse = (id, error) => {
  const { code, retryable } = errorClass(error);
  return response(id, "error", {
    error: { code, message: String(error?.message || error), retryable },
  });
};

/** True for a JSON object that is not an array. */
export const isPlainObject = (value) =>
  value !== null && typeof value === "object" && !Array.isArray(value);

const parseJson = (line) => {
  try {
    return JSON.parse(line);
  } catch {
    return undefined;
  }
};

const invalid = (id, code, message) => ({
  ok: false,
  id,
  error: new CompanionError(code, message),
});

/**
 * Gives the first fault of a parsed request, or null. The checks run in
 * order, so the id of the error response is null until the id is valid.
 */
const requestFault = (value) => {
  if (value === undefined)
    return invalid(null, "bad-request", "the request line is not valid JSON");
  if (!isPlainObject(value))
    return invalid(null, "bad-request", "the request is not a JSON object");
  if (!Number.isSafeInteger(value.id)) {
    return invalid(null, "bad-request", "the request id is not an integer");
  }
  return null;
};

const versionFault = (value) =>
  value.v === WIRE_PROTOCOL_VERSION
    ? null
    : invalid(
        value.id,
        "protocol-version",
        `the companion speaks wire version ${WIRE_PROTOCOL_VERSION}, not ${JSON.stringify(value.v)}`,
      );

const typeFault = (value) =>
  REQUEST_TYPES.includes(value.type)
    ? null
    : invalid(value.id, "bad-request", `unknown request type: ${JSON.stringify(value.type)}`);

/**
 * Reads one request line. The result is `{ ok: true, request }`, or
 * `{ ok: false, id, error }` with the id to copy into the error response.
 */
export const parseRequestLine = (line) => {
  const value = parseJson(line);
  const fault = requestFault(value) ?? versionFault(value) ?? typeFault(value);
  return fault ?? { ok: true, request: value };
};

/** Throws a `bad-request` error when `condition` is false. */
export const expectRequest = (condition, message) => {
  if (!condition) {
    throw new CompanionError("bad-request", message);
  }
};
