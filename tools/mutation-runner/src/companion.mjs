import { createInterface } from "node:readline";

import { RUNNERS, SESSION_TOKEN_ENV } from "./generated-protocol.mjs";
import {
  CompanionError,
  errorResponse,
  expectRequest,
  isPlainObject,
  parseRequestLine,
  response,
} from "./protocol.mjs";

const SHUTDOWN_GRACE_MS = 4000;
const SESSION_TOKEN = /^[0-9A-Za-z]{16,128}$/u;
const EXIT_NO_SESSION_TOKEN = 2;

/**
 * Reads the session token and removes it from the environment, so that the
 * runner and the user code never inherit it. Null when it is missing or
 * malformed.
 */
const takeSessionToken = () => {
  const token = process.env[SESSION_TOKEN_ENV];
  delete process.env[SESSION_TOKEN_ENV];
  return SESSION_TOKEN.test(token ?? "") ? token : null;
};

// Each protocol line starts with the session token. A raw write to fd 1 (a
// worker thread, a `globalSetup`, a logger that calls `fs.writeSync(1)`) has
// no token, so the API reads it as stray output. Output that reaches
// `process.stdout` goes to stderr, where the API keeps a bounded tail.
const writeProtocol = process.stdout.write.bind(process.stdout);
process.stdout.write = process.stderr.write.bind(process.stderr);

const createSender = (token) => (message) =>
  new Promise((resolve) => {
    writeProtocol(`${token} ${JSON.stringify(message)}\n`, () => resolve());
  });

const loadAdapter = async (runner) => {
  switch (runner) {
    case "vitest":
      return import("./adapters/vitest.mjs");
    case "node-test":
      return import("./adapters/node-test.mjs");
    case "bun-test":
      return import("./adapters/bun-test.mjs");
    default:
      throw new CompanionError("bad-request", `no adapter for runner ${JSON.stringify(runner)}`);
  }
};

const withDeadline = (promise, milliseconds) =>
  Promise.race([promise, new Promise((resolve) => setTimeout(resolve, milliseconds).unref())]);

const createSession = () => {
  let adapter = null;

  const requireAdapter = () => {
    if (adapter === null) {
      throw new CompanionError("bad-request", "the companion has no runner; send init first");
    }
    return adapter;
  };

  const init = async (request) => {
    expectRequest(adapter === null, "init was already sent");
    expectRequest(
      RUNNERS.some((runner) => runner.name === request.runner),
      `unsupported runner: ${JSON.stringify(request.runner)}`,
    );
    expectRequest(
      request.options === undefined || isPlainObject(request.options),
      "options must be an object",
    );
    const module = await loadAdapter(request.runner);
    const created = await module.createAdapter(request);
    adapter = created;
    return response(request.id, "ready", {
      runner_version: created.runnerVersion,
      runtime_version: process.version,
      test_files: created.testFiles,
      capabilities: created.capabilities,
      notes: created.notes,
    });
  };

  const handlers = {
    init,
    dry_run: async (request) =>
      response(request.id, "dry_run_result", await requireAdapter().dryRun(request)),
    run: async (request) => response(request.id, "run_result", await requireAdapter().run(request)),
    reload: async (request) =>
      response(request.id, "reloaded", await requireAdapter().reload(request)),
  };

  const close = async () => {
    if (adapter !== null) {
      await withDeadline(
        adapter.close().catch(() => {}),
        SHUTDOWN_GRACE_MS,
      );
    }
  };

  return { handlers, close };
};

const main = () => {
  const token = takeSessionToken();
  if (token === null) {
    process.stderr.write(`the companion needs a session token in ${SESSION_TOKEN_ENV}\n`);
    process.exit(EXIT_NO_SESSION_TOKEN);
  }
  const send = createSender(token);
  const session = createSession();
  let queue = Promise.resolve();
  let stopping = false;

  const stop = async () => {
    if (stopping) return;
    stopping = true;
    await session.close();
    process.exit(0);
  };

  const respond = async (request) => {
    try {
      return await session.handlers[request.type](request);
    } catch (error) {
      return errorResponse(request.id, error);
    }
  };

  const handleLine = async (line) => {
    if (stopping) return;
    const parsed = parseRequestLine(line);
    if (!parsed.ok) {
      await send(errorResponse(parsed.id, parsed.error));
    } else if (parsed.request.type === "shutdown") {
      await stop();
    } else {
      await send(await respond(parsed.request));
    }
  };

  const input = createInterface({ input: process.stdin, crlfDelay: Infinity });
  input.on("line", (line) => {
    queue = queue.then(() => handleLine(line));
  });
  input.on("close", () => {
    queue = queue.then(stop);
  });
};

main();
