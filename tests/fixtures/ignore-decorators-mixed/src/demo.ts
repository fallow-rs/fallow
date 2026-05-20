import { Inject, SomeService, step } from "./decorators";

export class Demo {
  @step("x")
  decoratedOnly() {}

  @step("y")
  @Inject(SomeService)
  mixed() {}

  @Inject(SomeService)
  frameworkOnly() {}

  plainUnused() {}

  @step("z")
  actuallyUsed() {
    return "used";
  }
}
