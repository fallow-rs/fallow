import { SharedBase } from "./shared-base";

export class CrossFileRoom extends SharedBase {
  async webSocketMessage(ws: WebSocket, message: string): Promise<void> {}
}
