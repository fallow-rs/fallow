import { Component } from "@angular/core";

// An ATTRIBUTE selector (`[appAttr]`), not an element selector. Out of the
// first-cut scope (element selectors only), so it is never flagged even though
// `appAttr` is applied in no template.
@Component({
  selector: "[appAttr]",
  template: `<span>attr</span>`,
})
export class AttrComponent {}
