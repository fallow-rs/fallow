import { DerivedClient } from "./derived-client";
import { DerivedService } from "./derived-service";
import { PlainClient } from "./plain-client";
import { PlainDerivedService } from "./plain-derived-service";

async function main(): Promise<void> {
  const service = new DerivedService(new DerivedClient());
  console.log(await service.fetchSyntheticRecords());

  const plain = new PlainDerivedService(new PlainClient());
  console.log(await plain.run());
}

void main();
