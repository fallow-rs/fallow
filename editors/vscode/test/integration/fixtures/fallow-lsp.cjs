#!/usr/bin/env node
let buffer = Buffer.alloc(0);
const send = (message) => {
  const payload = JSON.stringify(message);
  process.stdout.write(`Content-Length: ${Buffer.byteLength(payload, "utf8")}\r\n\r\n${payload}`);
};
const handle = (message) => {
  if (message.method === "initialize") {
    send({ jsonrpc: "2.0", id: message.id, result: { capabilities: {} } });
    return;
  }
  if (message.method === "shutdown") {
    send({ jsonrpc: "2.0", id: message.id, result: null });
    return;
  }
  if (message.id !== undefined) {
    send({
      jsonrpc: "2.0",
      id: message.id,
      error: { code: -32601, message: "Method not found" },
    });
    return;
  }
  if (message.method === "exit") {
    process.exit(0);
  }
};
process.stdin.on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  while (true) {
    const headerEnd = buffer.indexOf("\r\n\r\n");
    if (headerEnd === -1) {
      return;
    }
    const header = buffer.subarray(0, headerEnd).toString("ascii");
    const match = header.match(/Content-Length: (\d+)/i);
    if (!match) {
      process.exit(1);
    }
    const contentLength = Number(match[1]);
    const messageStart = headerEnd + 4;
    if (buffer.length < messageStart + contentLength) {
      return;
    }
    const payload = buffer.subarray(messageStart, messageStart + contentLength).toString("utf8");
    buffer = buffer.subarray(messageStart + contentLength);
    handle(JSON.parse(payload));
  }
});
