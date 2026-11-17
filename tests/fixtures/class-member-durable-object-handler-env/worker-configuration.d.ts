// The shape of the file that `wrangler types` writes.
interface __BaseEnv_Env {
  ROOM: DurableObjectNamespace<import('./src/room').Room>;
}
declare namespace Cloudflare {
  interface Env extends __BaseEnv_Env {}
}
interface Env extends __BaseEnv_Env {}
