import { PlainBaseService } from "./plain-base-service";

export class PlainDerivedService extends PlainBaseService {
  async run(): Promise<string> {
    return await this.client.plainUsed();
  }
}
