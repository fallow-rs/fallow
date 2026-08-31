import { json } from "@sveltejs/kit";

export const GET = () => json({ ok: true });

export const QUERY = () => json({ results: [] });
