import { Component } from "@angular/core";

@Component({
  selector: "safe-inline-root",
  template: `
    <section>
      <div [innerHTML]="'<strong>static</strong>'"></div>
    </section>
  `
})
export class SafeInlineComponent {}
