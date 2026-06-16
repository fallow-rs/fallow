import { Component, Input } from "@angular/core";

@Component({
  selector: "app-external-template",
  templateUrl: "./external-template.component.html",
})
export class ExternalTemplateComponent {
  // Read only in the external HTML template (cross-file credit).
  @Input() title = "";
}
