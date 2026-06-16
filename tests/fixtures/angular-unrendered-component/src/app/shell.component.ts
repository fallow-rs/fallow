import { Component } from "@angular/core";
import { UsedComponent } from "./used.component";
import { OrphanComponent } from "./orphan.component";
import { AttrComponent } from "./attr.component";

// `ShellComponent` registers OrphanComponent and AttrComponent in `imports:`
// (so they are reachable in the graph) but renders ONLY `<app-used>`. A bare
// `imports: [...]` registration is NOT a render, so the orphan's selector is
// used in NO template project-wide.
@Component({
  selector: "app-shell",
  imports: [UsedComponent, OrphanComponent, AttrComponent],
  template: `<app-used />`,
})
export class ShellComponent {}
