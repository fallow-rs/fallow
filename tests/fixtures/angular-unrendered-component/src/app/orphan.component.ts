import { Component } from "@angular/core";

// Registered in ShellComponent's `imports: [...]` (so it is reachable) but its
// `<app-orphan>` selector is rendered in NO template, it is not routed, not
// bootstrapped, and not dynamically rendered: the dead case this rule catches.
@Component({
  selector: "app-orphan",
  template: `<span>orphan</span>`,
})
export class OrphanComponent {}
