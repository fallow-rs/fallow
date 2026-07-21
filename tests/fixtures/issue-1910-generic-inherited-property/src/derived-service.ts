import { BaseService } from "./base-service";
import { DerivedClient } from "./derived-client";

export class DerivedService extends BaseService<DerivedClient> {
  async fetchSyntheticRecords(): Promise<string> {
    return await this.client.getSyntheticRecords();
  }
}
