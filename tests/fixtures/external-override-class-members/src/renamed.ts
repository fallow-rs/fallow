import { Container } from "@acme/containers";

class InnerContainer extends Container {
  override onStop(): void {}
  innerHelper(): void {}
}

export { InnerContainer as RenamedContainer };
