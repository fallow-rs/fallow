import { DeclarationFormClient } from "./declaration-form-client";

class SeparateFormService {
  constructor(public readonly client: DeclarationFormClient) {}

  run(): string {
    return this.client.separateForm();
  }
}

export { SeparateFormService };
