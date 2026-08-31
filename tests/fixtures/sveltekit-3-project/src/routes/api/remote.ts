import { query } from "$app/server";

export const health = query(async () => ({ ok: true }));
