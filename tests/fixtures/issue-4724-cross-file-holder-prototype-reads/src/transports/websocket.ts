export class WebSocketTransport {
  get handlesUpgrades(): boolean {
    return true;
  }

  unusedWebSocketHelper(): void {}
}
