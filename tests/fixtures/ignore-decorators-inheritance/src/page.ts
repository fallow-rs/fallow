import { step } from "./decorators";

export class Page {
  @step("base run")
  run() {}
}

export class AdminPage extends Page {}
