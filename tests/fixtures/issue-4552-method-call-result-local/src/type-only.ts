import type { Point } from "./point.js";

export function shiftTypeOnly(local: Point, center: Point, angle: number): Point {
    const rotated = angle ? local.rotate(angle) : local;
    return rotated.shift(center.x);
}

export function skewTypeOnly(local: Point, angle: number): Point {
    const rotated = local.rotate(angle);
    return rotated.skew(angle);
}

export function tiltTypeOnly(local: Point | undefined, angle: number): Point | undefined {
    const rotated = local?.rotate(angle);
    return rotated?.tilt(angle);
}
