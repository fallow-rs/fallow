import { Component, EventEmitter, Output } from "@angular/core";

@Component({
  selector: "app-emitted-output",
  template: `<button (click)="fire()">emit</button>`,
})
export class EmittedOutputComponent {
  // Emitted via `this.saved.emit(...)`, so it is credited and never flagged.
  @Output() saved = new EventEmitter<void>();

  fire(): void {
    this.saved.emit();
  }
}
