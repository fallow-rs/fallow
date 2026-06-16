import type { Routes } from "@angular/router";
import { RoutedComponent } from "./routed.component";

export const routes: Routes = [
  { path: "routed", component: RoutedComponent },
  {
    path: "lazy",
    loadComponent: () =>
      import("./lazy.component").then((m) => m.LazyComponent),
  },
  {
    // Bare loadComponent form (no `.then`): loads the module's default export.
    path: "bare-lazy",
    loadComponent: () => import("./bare-lazy.component"),
  },
];
