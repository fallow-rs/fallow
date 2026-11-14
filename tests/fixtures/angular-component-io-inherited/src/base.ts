import { Directive, input, output } from "@angular/core";

@Directive()
export abstract class Base {
  readonly readInSubclassTs = input("");
  readonly readInSubclassTemplate = input("");
  readonly emittedBySubclass = output<void>();
  readonly readByGrandChild = input("");
  readonly emittedByGrandChild = output<void>();
  readonly readOnlyByUnrelated = input("");
  readonly neverRead = input("");
  readonly neverEmitted = output<void>();
}
