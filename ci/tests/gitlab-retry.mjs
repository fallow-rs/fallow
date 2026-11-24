import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createServer } from "node:http";
import { once } from "node:events";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const run = promisify(execFile);
const helper = fileURLToPath(new URL("../scripts/gitlab_common.sh", import.meta.url));
for (const status of [503, 403]) {
  let attempts = 0;
  const server = createServer((_request, response) => {
    attempts += 1;
    response.writeHead(attempts === 1 ? status : 200);
    response.end(attempts === 1 ? "error" : "metadata");
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  try {
    const url = `http://127.0.0.1:${server.address().port}/merge_requests/1`;
    const result = run("bash", ["-c", 'source "$1"; curl_retry "$2"', "test", helper, url], {
      env: { ...process.env, FALLOW_API_RETRIES: "2", FALLOW_API_RETRY_DELAY: "0" },
    });
    if (status === 503) {
      assert.equal((await result).stdout, "metadata");
      assert.equal(attempts, 2);
    } else {
      await assert.rejects(result, { code: 1 });
      assert.equal(attempts, 1);
    }
  } finally {
    server.close();
  }
}
