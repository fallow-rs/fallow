import { defineParams } from "@sveltejs/kit/params";

export const params = defineParams({
  slug: (param: string) => (/^[a-z-]+$/.test(param) ? param : undefined),
  id: (param: string) => (/^\d+$/.test(param) ? param : undefined),
});
