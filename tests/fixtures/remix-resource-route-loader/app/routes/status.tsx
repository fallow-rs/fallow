import { useLoaderData } from "@remix-run/react";

export const loader = async () => {
  return Response.json({ shown: "ok", hidden: "secret" });
};

export default function Status() {
  const { shown } = useLoaderData<typeof loader>();
  return <p>{shown}</p>;
}
