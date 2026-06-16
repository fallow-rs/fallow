import { Component } from "@angular/core";
import { DeadIoComponent } from "./dead-io.component";
import { TemplateUsedComponent } from "./template-used.component";
import { ScriptUsedComponent } from "./script-used.component";
import { ExternalTemplateComponent } from "./external-template.component";
import { InputsArrayComponent } from "./inputs-array.component";
import { ExtendsComponent } from "./extends.component";
import { EmittedOutputComponent } from "./emitted-output.component";
import { TemplateEmitOutputComponent } from "./template-emit-output.component";
import { SpreadThisComponent } from "./spread-this.component";
import { ObservableOutputComponent } from "./observable-output.component";
import { SignalIoComponent } from "./signal-io.component";

@Component({
  selector: "app-root",
  imports: [
    DeadIoComponent,
    TemplateUsedComponent,
    ScriptUsedComponent,
    ExternalTemplateComponent,
    InputsArrayComponent,
    ExtendsComponent,
    EmittedOutputComponent,
    TemplateEmitOutputComponent,
    SpreadThisComponent,
    ObservableOutputComponent,
    SignalIoComponent,
  ],
  template: `
    <app-dead-io />
    <app-template-used />
    <app-script-used />
    <app-external-template />
    <app-inputs-array />
    <app-extends />
    <app-emitted-output />
    <app-template-emit-output />
    <app-spread-this />
    <app-observable-output />
    <app-signal-io />
  `,
})
export class AppComponent {}
