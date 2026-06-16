import { Directive } from "@angular/core";

@Directive()
export abstract class BaseComponent {
  // A base class can read a child's input through `this.foo`, invisible to the
  // per-module scan, so a child that extends this abstains entirely.
  protected describe(): string {
    return "base";
  }
}
