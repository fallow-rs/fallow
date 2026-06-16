import { Component } from "@angular/core";

@Component({
  selector: "app-inputs-array",
  // `mode` is declared as framework-managed via the decorator `inputs:` array;
  // the extractor sentinel-credits it, so it must never be flagged even though
  // no template ref or `this.mode` access exists.
  inputs: ["mode"],
  template: `<div>inputs-array</div>`,
})
export class InputsArrayComponent {
  mode = "default";
}
