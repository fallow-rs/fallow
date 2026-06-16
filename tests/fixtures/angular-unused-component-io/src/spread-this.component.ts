import { Component, EventEmitter, Input, Output, input, output } from "@angular/core";

@Component({
  selector: "app-spread-this",
  template: `<div>spread-this</div>`,
})
export class SpreadThisComponent {
  // Neither is read/emitted by name, but `{ ...this }` forwards every member
  // opaquely into a behavior pattern, so the whole component must abstain (the
  // Angular headless-pattern convention).
  @Input() forwardedInput = "";
  @Output() forwardedOutput = new EventEmitter<void>();
  readonly forwardedSignalInput = input<number>(0);
  readonly forwardedSignalOutput = output<void>();

  readonly pattern = { ...this, extra: 1 };
}
