import { DeclarationFormClient } from "./declaration-form-client";

export default class {
  constructor(public readonly client: DeclarationFormClient) {}

  run(): string {
    return this.client.anonymousDefaultForm();
  }
}
