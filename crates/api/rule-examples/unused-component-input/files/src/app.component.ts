import { Component } from "@angular/core";
import { UserCardComponent } from "./user-card.component";

@Component({
  selector: "app-root",
  imports: [UserCardComponent],
  template: `<app-user-card name="Ada" size="large" />`,
})
export class AppComponent {}
