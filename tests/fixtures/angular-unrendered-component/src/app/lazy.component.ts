import { Component } from "@angular/core";

// Lazily routed via `loadComponent: () => import('./lazy.component').then(m =>
// m.LazyComponent)` (see app.routes.ts): a render-equivalent entry point,
// abstained even though `<app-lazy>` appears in no template.
@Component({
  selector: "app-lazy",
  template: `<span>lazy</span>`,
})
export class LazyComponent {}
