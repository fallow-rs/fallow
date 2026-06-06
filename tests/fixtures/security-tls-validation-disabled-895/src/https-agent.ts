import { Agent } from "https";

export const agent = new Agent({
  rejectUnauthorized: false,
});
