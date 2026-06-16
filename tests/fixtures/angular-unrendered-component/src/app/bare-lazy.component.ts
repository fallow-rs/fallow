import { Component } from "@angular/core";

// Lazily routed via the BARE `loadComponent: () => import('./bare-lazy.component')`
// form (NO `.then(m => m.X)`, see app.routes.ts), which loads this module's
// `export default class`. The route config carries no class name, so the only
// render-equivalence signal is the DEFAULT-export dynamic-import credit. Must be
// abstained even though `<app-bare-lazy>` appears in no template (regression for
// the angular-realworld bare-loadComponent false positive).
@Component({
  selector: "app-bare-lazy",
  template: `<span>bare lazy</span>`,
})
export default class BareLazyComponent {}
