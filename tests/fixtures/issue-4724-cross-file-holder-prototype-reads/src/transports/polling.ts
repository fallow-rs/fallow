export class PollingTransport {
  get handlesUpgrades(): boolean {
    return false;
  }

  unusedPollingHelper(): void {}
}
