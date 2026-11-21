import { redirect } from "react-router";

export async function loader({ request }: { request: Request }) {
  const next = new URL(request.url).searchParams.get("next") ?? "/";
  return redirect(next);
}

export default function Page() {
  return null;
}
