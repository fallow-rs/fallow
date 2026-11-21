import { replace } from "react-router";

export async function action({ request }: { request: Request }) {
  const next = new URL(request.url).searchParams.get("next") ?? "/";
  return replace(next);
}

export default function Page() {
  return null;
}
