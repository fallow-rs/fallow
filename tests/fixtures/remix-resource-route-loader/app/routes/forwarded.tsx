import { json } from "@remix-run/node";

export const loader = async () => {
  return json({ forwardedDead: "remove" });
};

export { default } from "../components/forwarded-view";
