export class DeclarationFormClient {
  separateForm(): string {
    return "separate";
  }

  namedExportForm(): string {
    return "named-export";
  }

  namedDefaultForm(): string {
    return "named-default";
  }

  anonymousDefaultForm(): string {
    return "anonymous-default";
  }

  deadFormControl(): string {
    return "dead";
  }
}
