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

    scale(f: number): Point {
        return new Point(this.x * f, this.y * f);
    }

    mirror(): Point {
        return new Point(-this.x, -this.y);
    }

    flip(): Point {
        return new Point(this.y, this.x);
    }

    shift(dx: number): Point {
        return new Point(this.x + dx, this.y);
    }

    skew(dy: number): Point {
        return new Point(this.x, this.y + dy);
    }

    nudge(dx: number): Point {
        return new Point(this.x + dx, this.y + dx);
    }

    tilt(dy: number): Point {
        return new Point(this.x - dy, this.y + dy);
    }

    unused(): number {
        return 0;
    }
}
