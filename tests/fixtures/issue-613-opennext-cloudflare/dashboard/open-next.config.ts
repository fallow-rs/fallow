import { defineCloudflareConfig } from "@opennextjs/cloudflare";
import { durableIncrementalCache } from "@opennextjs/aws";

export default defineCloudflareConfig({
  incrementalCache: durableIncrementalCache,
});
