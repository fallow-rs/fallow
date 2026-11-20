import { Component } from "@angular/core";
import { UserCardComponent } from "./user-card.component";

@Component({
  selector: "app-root",
  imports: [UserCardComponent],
  template: `<app-user-card (save)="onSave()" />`,
})
export class AppComponent {
  onSave(): void {
    console.log("user saved");
  }
}
