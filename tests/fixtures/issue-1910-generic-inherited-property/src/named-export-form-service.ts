import { DeclarationFormClient } from "./declaration-form-client";

export class NamedExportFormService {
  constructor(public readonly client: DeclarationFormClient) {}

  run(): string {
    return this.client.namedExportForm();
  }
}
