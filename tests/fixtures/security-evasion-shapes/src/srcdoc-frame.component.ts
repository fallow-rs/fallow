import { Component } from "@angular/core";

@Component({
  selector: "srcdoc-frame",
  template: `
    <iframe title="preview" [srcdoc]="previewHtml"></iframe>
  `
})
export class SrcdocFrameComponent {
  previewHtml = window.location.hash;
}
