import { decorators } from "./decorators";

export class Demo {
  @decorators.log()
  loggedMethod() {}

  @decorators.audit()
  auditedMethod() {}

  plainMethod() {}
}
