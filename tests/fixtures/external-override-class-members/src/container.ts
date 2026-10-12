import { Container } from "@acme/containers";

export class PortContainer extends Container {
  override defaultPort = 8080;
  override onStop(): void {}
  ownHelper(): void {}
}

export class ChildContainer extends PortContainer {
  override onStop(): void {}
  childHelper(): void {}
}
