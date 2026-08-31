import { defineParams } from "@sveltejs/kit/params";

export const slug = defineParams((param: string) => /^[a-z-]+$/.test(param));

export const id = defineParams((param: string) => /^\d+$/.test(param));
