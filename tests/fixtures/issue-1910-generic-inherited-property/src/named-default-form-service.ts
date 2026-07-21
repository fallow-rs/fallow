import { DeclarationFormClient } from "./declaration-form-client";

export default class NamedDefaultFormService {
  constructor(public readonly client: DeclarationFormClient) {}

  run(): string {
    return this.client.namedDefaultForm();
  }
}
