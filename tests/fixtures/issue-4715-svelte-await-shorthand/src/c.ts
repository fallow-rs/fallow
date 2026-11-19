export const delayShort = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));
export const delayLong = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));
export const loadValue = (): Promise<string> => Promise.resolve('value');
export const loadOrFail = (): Promise<string> => Promise.reject(new Error('fail'));
export const neverAwaited = (): Promise<void> => Promise.resolve();
