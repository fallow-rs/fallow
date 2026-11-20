import { Component, EventEmitter, Output } from "@angular/core";

@Component({
  selector: "app-user-card",
  template: `<button (click)="save.emit()">Save</button>`,
})
export class UserCardComponent {
  @Output() save = new EventEmitter<void>();
  @Output() close = new EventEmitter<void>();
}
