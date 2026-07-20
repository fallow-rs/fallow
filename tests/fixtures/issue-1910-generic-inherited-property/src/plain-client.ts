export class PlainClient {
  // Called via non-generic inherited `this.client.plainUsed()`. Must be CREDITED.
  async plainUsed(): Promise<string> {
    return "ok";
  }

  // Never called. Control - must STAY flagged.
  async plainDead(): Promise<string> {
    return "dead";
  }
}
