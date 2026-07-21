import AnonymousDefaultFormService from "./anonymous-default-form-service";
import { DeclarationFormClient } from "./declaration-form-client";
import { DerivedClient } from "./derived-client";
import { DeepDerivedService } from "./deep-derived-service";
import { DerivedService } from "./derived-service";
import NamedDefaultFormService from "./named-default-form-service";
import { NamedExportFormService } from "./named-export-form-service";
import { PlainClient } from "./plain-client";
import { PlainDerivedService } from "./plain-derived-service";
import { SeparateFormService } from "./separate-form-service";
import {
  CallingSiblingService,
  SilentSiblingService,
  UnusedSiblingClient,
  UsedSiblingClient,
} from "./sibling-services";
import { UnresolvedShadowService } from "./unresolved-shadow-service";

async function main(): Promise<void> {
  const service = new DerivedService(new DerivedClient());
  console.log(await service.fetchSyntheticRecords());

  const deep = new DeepDerivedService(new DerivedClient());
  console.log(await deep.fetchDeepRecords());

  const plain = new PlainDerivedService(new PlainClient());
  console.log(await plain.run());

  console.log(new CallingSiblingService(new UsedSiblingClient()).run());
  console.log(new SilentSiblingService(new UnusedSiblingClient()).keepAlive());

  const formClient = new DeclarationFormClient();
  console.log(new SeparateFormService(formClient).run());
  console.log(new NamedExportFormService(formClient).run());
  console.log(new NamedDefaultFormService(formClient).run());
  console.log(new AnonymousDefaultFormService(formClient).run());

  const unresolvedShadow = new UnresolvedShadowService(new DerivedClient());
  console.log(await unresolvedShadow.run());
}

void main();
