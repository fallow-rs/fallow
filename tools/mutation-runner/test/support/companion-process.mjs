import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline";

import { SESSION_TOKEN_ENV } from "../../src/generated-protocol.mjs";

const COMPANION = fileURLToPath(new URL("../../src/companion.mjs", import.meta.url));

/**
 * Starts the companion as a child process with a new session token, and
 * reads its stdout as the API does: a line that starts with the token is a
 * protocol line, and each other line is stray output. Both lists are kept, so
 * a test can prove that runner output never reaches the protocol channel.
 * `token: null` starts the companion with no token. `runtime` is the
 * executable that runs the companion, Node by default. `detached: true` starts
 * the companion in its own process group, as the API does on Unix.
 * `unsetEnv` names the variables of this process that the companion does not
 * get. `setEnv` gives the companion more variables, as a shell environment
 * does. `setEnv` applies after `unsetEnv`.
 */
export const startCompanion = ({
  token = randomBytes(16).toString("hex"),
  runtime = process.execPath,
  detached = false,
  unsetEnv = [],
  setEnv = {},
} = {}) => {
  const env = { ...process.env };
  for (const name of unsetEnv) delete env[name];
  Object.assign(env, setEnv);
  delete env[SESSION_TOKEN_ENV];
  if (token !== null) env[SESSION_TOKEN_ENV] = token;
  const child = spawn(runtime, [COMPANION], { stdio: ["pipe", "pipe", "pipe"], env, detached });
  const prefix = `${token} `;
  const lines = [];
  const strayLines = [];
  const waiting = [];
  let stderr = "";
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  createInterface({ input: child.stdout }).on("line", (rawLine) => {
    if (!rawLine.startsWith(prefix)) {
      strayLines.push(rawLine);
      return;
    }
    const line = rawLine.slice(prefix.length);
    lines.push(line);
    const next = waiting.shift();
    if (next) next.resolve(line);
  });
  let exitInfo = null;
  const exited = new Promise((resolve) => {
    child.on("exit", (code, signal) => {
      exitInfo = { code, signal };
      for (const reject of waiting.splice(0).map((entry) => entry.reject)) {
        reject(new Error(`companion exited (${code ?? signal}) before a response:\n${stderr}`));
      }
      resolve(exitInfo);
    });
  });
  let consumed = 0;
  const nextLine = () => {
    if (consumed < lines.length) {
      consumed += 1;
      return Promise.resolve(lines[consumed - 1]);
    }
    if (exitInfo) {
      return Promise.reject(new Error(`companion exited before a response:\n${stderr}`));
    }
    return new Promise((resolve, reject) => {
      waiting.push({
        resolve: (line) => {
          consumed += 1;
          resolve(line);
        },
        reject,
      });
    });
  };
  const sendRaw = (text) => child.stdin.write(`${text}\n`);
  const send = (message) => sendRaw(JSON.stringify(message));
  const request = async (message) => {
    send(message);
    return JSON.parse(await nextLine());
  };
  return {
    child,
    exited,
    lines,
    strayLines,
    nextLine,
    request,
    send,
    sendRaw,
    stderr: () => stderr,
    stop: () => {
      if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
    },
  };
};
