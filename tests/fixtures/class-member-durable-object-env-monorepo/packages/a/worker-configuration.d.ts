// The shape of the file that a current `wrangler types` writes. Both
// declarations of `Env` declare a binding.
declare namespace Cloudflare {
  interface Env {
    ROOM: DurableObjectNamespace<import('./src/room').RoomA>;
  }
}
interface Env extends Cloudflare.Env {
  LOBBY: DurableObjectNamespace<import('./src/room').LobbyA>;
}
