import { Component, input } from "@angular/core";

@Component({ selector: "x-inputs", templateUrl: "./inputs.component.html" })
export class Inputs {
  readonly pipeArg = input(0);
  readonly attrInterp = input("");
  readonly classInterp = input("");
  readonly tpl = input<unknown>(null);
  readonly outletCtx = input<object>({});
  readonly elseIfCond = input(false);
  readonly widthPct = input(0);
  readonly shadowed = input("");
  readonly items = input<string[]>([]);
  readonly icuCount = input(0);
  readonly neverRead = input("");
  n = 1;
}
