import { useLoaderData } from "react-router";

export async function loader() {
  return { used: "ok", dead: "remove", propOnly: "prop" };
}

export async function load() {
  return { pageOnly: "not a React Router loader key" };
}

export default function Home() {
  const data = useLoaderData<typeof loader>();
  return <Widget value={data.used} />;
}

export function Widget({ loaderData }: { loaderData: { propOnly: string } }) {
  return <span>{loaderData.propOnly}</span>;
}
