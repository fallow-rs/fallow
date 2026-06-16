import { Component, EventEmitter, Input, Output } from "@angular/core";

@Component({
  selector: "app-dead-io",
  template: `<div>dead-io</div>`,
})
export class DeadIoComponent {
  // Declared but read nowhere (no template ref, no this.deadInput).
  @Input() deadInput = "";

  // Declared but never .emit()-ed anywhere.
  @Output() deadOutput = new EventEmitter<void>();
}
