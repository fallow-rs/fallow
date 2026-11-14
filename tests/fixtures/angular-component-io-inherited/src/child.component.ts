import { Component } from "@angular/core";
import { Base } from "./base";

@Component({
  selector: "x-child",
  template: "<p>{{ readInSubclassTemplate() }}</p>",
})
export class Child extends Base {
  log(): string {
    this.emittedBySubclass.emit();
    return this.readInSubclassTs();
  }
}
