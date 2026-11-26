// Reads the LSP traffic of a watch-loop run and checks it against limits.
// `watch-loop-harness.mjs` drives VS Code. This module has no side effects,
// so `watch-loop-events.test.mjs` can test it without an editor.

/**
 * Parse the LSP frames in the bytes of one stream direction.
 * @param {Buffer} buffer
 * @returns {Array<Record<string, any>>}
 */
export const parseFrames = (buffer) => {
  const messages = [];
  let offset = 0;
  while (offset < buffer.length) {
    const headerEnd = buffer.indexOf("\r\n\r\n", offset);
    if (headerEnd < 0) break;
    const header = buffer.subarray(offset, headerEnd).toString("latin1");
    const match = /Content-Length: *(\d+)/i.exec(header);
    if (!match) {
      // A restarted server leaves a cut frame. Continue at the next header.
      const next = buffer.indexOf("Content-Length:", offset + 1);
      if (next < 0) break;
      offset = next;
      continue;
    }
    const start = headerEnd + 4;
    const end = start + Number(match[1]);
    if (end > buffer.length) break;
    try {
      messages.push(JSON.parse(buffer.subarray(start, end).toString("utf8")));
    } catch {
      // The body is not complete JSON. Skip it.
    }
    offset = end;
  }
  return messages;
};

/**
 * Get the events of the two stream directions that the wrapper copied.
 * @param {Buffer} clientToServer
 * @param {Buffer} serverToClient
 */
export const eventsFromWire = (clientToServer, serverToClient) => {
  const events = [];
  for (const message of parseFrames(clientToServer)) {
    if (message.method === "workspace/didChangeWatchedFiles") {
      const uris = message.params.changes.map((change) => `${change.type} ${change.uri}`);
      events.push({ kind: "watched", uris });
    } else if (message.method === "textDocument/didSave") {
      events.push({ kind: "didSave", uris: [message.params.textDocument.uri] });
    }
  }
  for (const message of parseFrames(serverToClient)) {
    const text =
      message.method === "window/logMessage" ? String(message.params?.message ?? "") : "";
    if (message.method === "fallow/analysisComplete") {
      events.push({ kind: "complete", total: message.params.totalIssues });
    } else if (text.includes("Running fallow analysis")) {
      events.push({ kind: "running" });
    } else if (text.includes("Cancelled")) {
      events.push({ kind: "cancelled", text });
    }
  }
  return events;
};

const TRACE_ENTRY_START = /\r?\n(?=\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d{3} \[)/;

/**
 * Get the events from the text of a Fallow Output channel log that VS Code
 * wrote with `fallow.trace.server` set to `verbose`.
 * @param {string} text
 */
export const eventsFromTrace = (text) => {
  const events = [];
  for (const entry of text.split(TRACE_ENTRY_START)) {
    if (entry.includes("Sending notification 'workspace/didChangeWatchedFiles'")) {
      const uris = [...entry.matchAll(/"uri":\s*"([^"]+)"[\s\S]*?"type":\s*(\d)/g)].map(
        (match) => `${match[2]} ${match[1]}`,
      );
      events.push({ kind: "watched", uris });
    } else if (entry.includes("Sending notification 'textDocument/didSave'")) {
      events.push({ kind: "didSave", uris: [] });
    } else if (entry.includes("Received notification 'fallow/analysisComplete'")) {
      const total = /"totalIssues":\s*(\d+)/.exec(entry);
      events.push({ kind: "complete", total: total ? Number(total[1]) : null });
    } else if (/\] Running fallow analysis/.test(entry)) {
      events.push({ kind: "running" });
    } else if (/\] Cancelled a fallow analysis/.test(entry)) {
      events.push({ kind: "cancelled", text: entry });
    }
  }
  return events;
};

/** Count the lines that the Output channel labels `[error]`. */
export const countErrorLines = (text) =>
  text.split(/\r?\n/).filter((line) => /\[error\]/.test(line)).length;

/** Count the events of one kind. */
export const countKind = (events, kind) => events.filter((event) => event.kind === kind).length;

/**
 * Return a filter that keeps the events after a mark. The event list joins
 * the two stream directions, so the events after the mark are the events of
 * each kind beyond the count of that kind at the mark.
 */
export const eventsAfter = (eventsAtMark) => {
  const marked = {};
  for (const event of eventsAtMark) marked[event.kind] = (marked[event.kind] ?? 0) + 1;
  return (events) => {
    const seen = {};
    return events.filter((event) => {
      seen[event.kind] = (seen[event.kind] ?? 0) + 1;
      return seen[event.kind] > (marked[event.kind] ?? 0);
    });
  };
};

/**
 * Compare the counts after the save with the limits. Returns one message for
 * each count over its limit. A limit that is `undefined` is not checked.
 * @param {{analysisRuns: number, cancelled: number, errorLines: number}} counts
 * @param {{maxRuns?: number, maxCancels?: number, maxErrorLines?: number}} limits
 */
export const limitFailures = (counts, limits) => {
  const checks = [
    ["analysis runs after the save", counts.analysisRuns, limits.maxRuns],
    ["cancelled runs after the save", counts.cancelled, limits.maxCancels],
    ["[error] lines in the Output channel", counts.errorLines, limits.maxErrorLines],
  ];
  return checks
    .filter(([, value, limit]) => limit !== undefined && value > limit)
    .map(([name, value, limit]) => `${name}: ${value}, limit ${limit}`);
};
