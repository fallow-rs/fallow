import assert from "node:assert/strict";
import { test } from "node:test";

import {
  countErrorLines,
  countKind,
  eventsAfter,
  eventsFromTrace,
  eventsFromWire,
  limitFailures,
  parseFrames,
} from "./watch-loop-events.mjs";

const frame = (message) => {
  const body = Buffer.from(JSON.stringify(message), "utf8");
  return Buffer.concat([Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, "latin1"), body]);
};

const watched = (uri) => ({
  jsonrpc: "2.0",
  method: "workspace/didChangeWatchedFiles",
  params: { changes: [{ uri, type: 2 }] },
});

const logMessage = (message) => ({
  jsonrpc: "2.0",
  method: "window/logMessage",
  params: { type: 3, message },
});

test("parseFrames reads frames with a multibyte body and skips a cut frame", () => {
  const cut = Buffer.from("Content-Type: x\r\n\r\n{", "latin1");
  const buffer = Buffer.concat([
    frame({ text: "café" }),
    cut,
    frame({ n: 2 }),
    Buffer.from("Content-Length: 99\r\n\r\n{}", "latin1"),
  ]);
  assert.deepEqual(parseFrames(buffer), [{ text: "café" }, { n: 2 }]);
});

test("eventsFromWire gets each event kind from the two directions", () => {
  const clientToServer = Buffer.concat([
    frame(watched("file:///ws/.fallowrc.json")),
    frame({ method: "textDocument/didSave", params: { textDocument: { uri: "file:///ws/a.ts" } } }),
  ]);
  const serverToClient = Buffer.concat([
    frame(logMessage("Running fallow analysis")),
    frame(logMessage("Cancelled a fallow analysis")),
    frame({ method: "fallow/analysisComplete", params: { totalIssues: 3 } }),
  ]);
  const events = eventsFromWire(clientToServer, serverToClient);
  assert.deepEqual(
    events.map((event) => event.kind),
    ["watched", "didSave", "running", "cancelled", "complete"],
  );
  assert.deepEqual(events[0].uris, ["2 file:///ws/.fallowrc.json"]);
  assert.equal(events.at(-1).total, 3);
});

test("eventsFromTrace reads a multiline trace entry", () => {
  const text = [
    "2026-11-25 10:00:00.000 [trace] Sending notification 'workspace/didChangeWatchedFiles'.",
    'Params: {"changes": [{"uri": "file:///ws/.fallowrc.json", "type": 2}]}',
    "2026-11-25 10:00:01.000 [info] Running fallow analysis",
    "2026-11-25 10:00:02.000 [trace] Received notification 'fallow/analysisComplete'.",
    'Params: {"totalIssues": 7}',
  ].join("\n");
  const events = eventsFromTrace(text);
  assert.deepEqual(
    events.map((event) => event.kind),
    ["watched", "running", "complete"],
  );
  assert.deepEqual(events[0].uris, ["2 file:///ws/.fallowrc.json"]);
  assert.equal(events[2].total, 7);
});

test("eventsAfter keeps the events of each kind beyond the count at the mark", () => {
  const atMark = [{ kind: "running" }, { kind: "complete" }];
  const later = [...atMark, { kind: "watched" }, { kind: "running" }, { kind: "complete" }];
  const after = eventsAfter(atMark)(later);
  assert.equal(countKind(after, "running"), 1);
  assert.equal(countKind(after, "complete"), 1);
  assert.equal(countKind(after, "watched"), 1);
});

test("countErrorLines counts the lines that VS Code labels as errors", () => {
  const text = "2026 [info] a\r\n2026 [error] INFO fallow_extract: b\n2026 [error] c";
  assert.equal(countErrorLines(text), 2);
});

test("limitFailures names each count over its limit and skips an unset limit", () => {
  const counts = { analysisRuns: 57, cancelled: 0, errorLines: 27 };
  assert.deepEqual(limitFailures(counts, { maxRuns: 2, maxCancels: 0 }), [
    "analysis runs after the save: 57, limit 2",
  ]);
  assert.deepEqual(limitFailures(counts, { maxRuns: 57, maxErrorLines: 27 }), []);
});
