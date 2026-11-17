import { env } from 'cloudflare:workers';

export { LobbyA, RoomA } from './room';

export default {
  async fetch(): Promise<Response> {
    const results = [
      await env.ROOM.getByName('a').viaA(),
      await env.LOBBY.getByName('a').viaLobby(),
    ];
    return new Response(String(results.every(Boolean)));
  },
};
