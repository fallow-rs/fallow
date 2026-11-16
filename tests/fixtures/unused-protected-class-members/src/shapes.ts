import { Base } from "./base.ts";

export class Shape extends Base {
  public area(): number {
    return this.scale() * 2 + this.render();
  }
  public unusedPublic(): number {
    return 1;
  }
  protected scale(): number {
    return 1;
  }
  protected unusedProtected(): number {
    return 2;
  }
  protected render(): number {
    return 0;
  }
  protected readonly unusedProtectedField = 3;
  // fallow-ignore-next-line unused-protected-class-member
  protected suppressedProtected(): number {
    return 5;
  }
}

export class Square extends Shape {
  public side(): number {
    return this.helper() + super.fromSuper();
  }
  protected helper(): number {
    return 3;
  }
  protected unusedHelper(): number {
    return 4;
  }
  protected testOnlyHelper(): number {
    return 8;
  }
  protected override render(): number {
    return 1;
  }
}

