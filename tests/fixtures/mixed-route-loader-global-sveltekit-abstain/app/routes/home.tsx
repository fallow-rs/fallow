import { useLoaderData } from "react-router";

export async function loader() {
  return { used: "ok", dead: "remove" };
}

export default function Home() {
  const data = useLoaderData<typeof loader>();
  return <h1>{data.used}</h1>;
}
