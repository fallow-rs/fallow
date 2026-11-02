export const run = (value: object, code: string): unknown => value.constructor.constructor(code);
