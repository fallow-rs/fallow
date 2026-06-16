import { Component, input, model, output } from "@angular/core";

@Component({
  selector: "app-signal-io",
  template: `<div>signal-io</div>`,
})
export class SignalIoComponent {
  // Signal input read nowhere -> flagged as unused-component-input.
  readonly size = input<number>(0);

  // Signal output emitted nowhere -> flagged as unused-component-output.
  readonly toggled = output<boolean>();

  // model() is recorded as an input only; unread -> flagged as input, and its
  // framework-driven `update:` emit is NEVER flagged as an output.
  readonly value = model<string>("");
}
