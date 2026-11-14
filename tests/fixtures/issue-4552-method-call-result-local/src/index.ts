import { Point } from "./point.js";

export { shiftTypeOnly, skewTypeOnly, tiltTypeOnly } from "./type-only.js";

export function viaParamTernary(local: Point, center: Point, angle: number): Point {
    const rotated = angle ? local.rotate(angle) : local;
    return rotated.translate(center.x, center.y);
}

export function viaParamDirect(local: Point, angle: number): Point {
    const rotated = local.rotate(angle);
    return rotated.scale(2);
}

export function viaNewLocalDirect(angle: number): Point {
    const local = new Point(0, 0);
    const rotated = local.rotate(angle);
    return rotated.mirror();
}

export function viaNewLocalTernary(angle: number): Point {
    const local = new Point(0, 0);
    const rotated = angle ? local.rotate(angle) : local;
    return rotated.flip();
}

export function viaParamOptional(local: Point | undefined, angle: number): Point | undefined {
    const rotated = local?.rotate(angle);
    return rotated?.nudge(angle);
}
