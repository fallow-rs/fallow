const e = eval;
export const direct = (code: string): unknown => e(code);
export const param = (e: (code: string) => void, code: string): void => { e(code); };
export const local = (code: string): string => { const e = (value: string): string => value; return e(code); };
export const caught = (code: string): void => { try { JSON.parse(code); } catch (e) { (e as (value: string) => void)(code); } };
export const later = (code: string): unknown => e(code);
