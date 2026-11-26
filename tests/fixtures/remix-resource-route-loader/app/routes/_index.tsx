import { json } from "@remix-run/node";
import { useLoaderData } from "@remix-run/react";

export const loader = async () => {
  return json({ used: "ok", dead: "remove" });
};

export default function Index() {
  const { used } = useLoaderData<typeof loader>();
  return <h1>{used}</h1>;
}
