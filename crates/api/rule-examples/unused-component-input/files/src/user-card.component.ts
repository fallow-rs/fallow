import { Component, Input } from "@angular/core";

@Component({
  selector: "app-user-card",
  template: `<p>{{ name }}</p>`,
})
export class UserCardComponent {
  @Input() name = "";
  @Input() size = "";
}
