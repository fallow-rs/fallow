const runInNewContext = (code: string): string => code.trim();
export const run = (code: string): string => runInNewContext(code);
