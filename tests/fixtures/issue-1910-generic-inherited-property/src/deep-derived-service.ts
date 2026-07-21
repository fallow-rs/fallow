import { DerivedClient } from "./derived-client";
import { IntermediateService } from "./intermediate-service";

export class DeepDerivedService extends IntermediateService<DerivedClient> {
  async fetchDeepRecords(): Promise<string> {
    return await this.client.getDeepRecords();
  }
}
