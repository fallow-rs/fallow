import { Component } from "@angular/core";

// Referenced as a route `component: RoutedComponent` (see app.routes.ts): a
// render-equivalent entry point, abstained even though `<app-routed>` appears in
// no template.
@Component({
  selector: "app-routed",
  template: `<span>routed</span>`,
})
export class RoutedComponent {}
