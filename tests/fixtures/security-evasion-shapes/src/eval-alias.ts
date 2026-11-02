export const run = (code: string): unknown => { const e = eval; return e(code); };
