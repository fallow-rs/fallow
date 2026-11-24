import { Link } from "react-router";

const redirect = (target: string): Response =>
  new Response(null, { status: 302, headers: { Location: `/app/${target}` } });

function replace(target: string): string {
  return target.trim();
}

export async function loader({ request }: { request: Request }) {
  const next = new URL(request.url).searchParams.get("next") ?? "/";
  replace(next);
  return redirect(next);
}

export default function Page() {
  return Link;
}
