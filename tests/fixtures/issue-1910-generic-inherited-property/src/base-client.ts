export abstract class BaseClient {
  // Genuinely unused: never called anywhere. Control - must STAY flagged.
  async inheritedMethod(): Promise<string> {
    return "base";
  }
}
