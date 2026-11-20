export abstract class Shape {
  abstract area(): number;

  protected unusedHelper(): string {
    return `Area: ${this.area()}`;
  }
}

export class Square extends Shape {
  constructor(private readonly side: number) {
    super();
  }

  area(): number {
    return this.side * this.side;
  }
}
