import { Component } from "@angular/core";
import { RouterOutlet } from "@angular/router";
import { ShellComponent } from "./shell.component";

// The bootstrapped root. Renders `<app-shell>` (which registers but does not
// render the orphan/attr components) plus the router outlet for routed pages.
@Component({
  selector: "app-root",
  imports: [RouterOutlet, ShellComponent],
  template: `
    <app-shell />
    <router-outlet />
  `,
})
export class AppComponent {}
