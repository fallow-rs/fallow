import { Component, Input } from "@angular/core";

@Component({
  selector: "app-template-used",
  // `label` is interpolated; `flag` is used in a property binding. Neither is
  // touched by the class body, so only the inline template credits them.
  template: `<div [hidden]="flag">{{ label }}</div>`,
})
export class TemplateUsedComponent {
  @Input() label = "";
  @Input() flag = false;
}
