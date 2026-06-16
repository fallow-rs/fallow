import { Component, EventEmitter, Input, Output } from "@angular/core";
import { BaseComponent } from "./base.component";

@Component({
  selector: "app-extends",
  template: `<div>extends</div>`,
})
export class ExtendsComponent extends BaseComponent {
  // Both look dead in this module, but the `extends` heritage clause abstains
  // the whole component (a base class may read/emit them cross-file).
  @Input() inheritedInput = "";
  @Output() inheritedOutput = new EventEmitter<void>();
}
