import { Component, Input } from "@angular/core";

@Component({
  selector: "app-script-used",
  template: `<div>script-used</div>`,
})
export class ScriptUsedComponent {
  // Read only via `this.count` in a method body; never in the template.
  @Input() count = 0;

  double(): number {
    return this.count * 2;
  }
}
