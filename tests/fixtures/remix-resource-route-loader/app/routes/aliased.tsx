import { json } from "@remix-run/node";
import { useLoaderData } from "@remix-run/react";

export const loader = async () => {
  return json({ aliasUsed: "ok", aliasDead: "remove" });
};

function Aliased() {
  const { aliasUsed } = useLoaderData<typeof loader>();
  return <p>{aliasUsed}</p>;
}

export { Aliased as default };
