import { request } from "../http";

export const load = (url: string): unknown => request(url);
