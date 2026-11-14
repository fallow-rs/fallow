import { Component } from "@angular/core";
import { Middle } from "./middle";

@Component({
  selector: "x-grand-child",
  templateUrl: "./grand-child.component.html",
})
export class GrandChild extends Middle {}
