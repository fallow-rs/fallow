export const invert = (data: Uint8ClampedArray, i: number, value: number): void => { data[i + 1] = 255 - value; };
