import { useLoaderData } from "react-router";

export async function loader() {
  return { dead: "ignored" };
}

export default function Home() {
  const data = useLoaderData<typeof loader>();
  return <span>{data.dead}</span>;
}
