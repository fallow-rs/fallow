const ORIGIN = "https://api.example.com";
const ALIAS = ORIGIN;

export const staticOrigin = (): Promise<Response> => fetch(ORIGIN);
export const staticAlias = (): Promise<Response> => fetch(ALIAS);
export const staticTemplate = (): Promise<Response> => fetch(`https://api.example.com`);
export const staticInterpolation = (): Promise<Response> => fetch(`${ORIGIN}`);
