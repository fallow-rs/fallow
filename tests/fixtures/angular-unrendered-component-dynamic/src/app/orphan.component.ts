import { Component } from "@angular/core";

// `<app-orphan>` is rendered in no template, but the project contains a
// `ViewContainerRef.createComponent(...)` dynamic render, so the whole project
// abstains and this is NOT flagged.
@Component({
  selector: "app-orphan",
  template: `<span>orphan</span>`,
})
export class OrphanComponent {}
