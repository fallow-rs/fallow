import { Component, EventEmitter, Output } from "@angular/core";

@Component({
  selector: "app-template-emit-output",
  // Emitted directly from a template handler off the bare name (no `this.`),
  // which is the canonical Angular emit shape. Must NOT be flagged.
  template: `<button (click)="picked.emit(true)">pick</button>`,
})
export class TemplateEmitOutputComponent {
  @Output() picked = new EventEmitter<boolean>();
}
