import { Component } from "@angular/core";

@Component({
  selector: "inline-root",
  template: `
    <section>
      <h1>Inline</h1>
      <div [innerHTML]="userHtml"></div>
    </section>
  `
})
export class InlineComponent {
  userHtml = window.location.hash;
}
