import { request } from "../net";

export const load = (url: string): unknown => request(url);
