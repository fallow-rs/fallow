import { Component, ViewContainerRef } from "@angular/core";
import { OrphanComponent } from "./orphan.component";

// This project renders a component DYNAMICALLY via
// `ViewContainerRef.createComponent(...)`, so a component could be instantiated
// from a non-literal class reference fallow cannot attribute. The Angular
// `unrendered-component` detector abstains on the WHOLE project: `OrphanComponent`
// (whose `<app-orphan>` appears in no template) is NOT flagged.
@Component({
  selector: "app-root",
  imports: [OrphanComponent],
  template: `<div #host></div>`,
})
export class AppComponent {
  constructor(private readonly vcr: ViewContainerRef) {}

  load(type: unknown): void {
    this.vcr.createComponent(type as never);
  }
}
