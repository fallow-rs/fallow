export class Point {
  constructor(
    readonly x: number,
    readonly y: number,
  ) {}

  rotate(angle: number): Point {
    return new Point(this.x * Math.cos(angle), this.y * Math.sin(angle));
  }

  translate(dx: number, dy: number): Point {
    return new Point(this.x + dx, this.y + dy);
  }

  scale(factor: number): Point {
    return new Point(this.x * factor, this.y * factor);
  }

  mirror(): Point {
    return new Point(-this.x, -this.y);
  }
}
