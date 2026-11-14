import { Component, output } from "@angular/core";

@Component({ selector: "x-outputs", templateUrl: "./outputs.component.html" })
export class Outputs {
  readonly beforeIcu = output();
  readonly afterIcu = output();
  readonly viaThis = output();
  readonly neverEmitted = output();
  n = 1;
}
