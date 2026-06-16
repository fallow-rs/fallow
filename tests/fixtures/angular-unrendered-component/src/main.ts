import { bootstrapApplication } from "@angular/platform-browser";
import { provideRouter } from "@angular/router";
import { AppComponent } from "./app/app.component";
import { routes } from "./app/app.routes";

// `AppComponent` is bootstrapped: a render-equivalent entry point (abstained).
bootstrapApplication(AppComponent, {
  providers: [provideRouter(routes)],
});
