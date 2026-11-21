import { redirectDocument } from "react-router";

export async function loader({ request }: { request: Request }) {
  const next = new URL(request.url).searchParams.get("next") ?? "/";
  return redirectDocument(next);
}

export default function Page() {
  return null;
}
