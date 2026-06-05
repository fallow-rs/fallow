import { Component } from "@angular/core";

@Component({
  selector: "pipe-view",
  templateUrl: "./pipe.component.html"
})
export class PipeComponent {
  mode = "strict";
  userHtml = window.location.hash;
}
