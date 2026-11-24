import { redirect } from "react-router";

export const loader = ({ request }: { request: Request }): Response => {
  const next = new URL(request.url).searchParams.get("next") ?? "/";
  return redirect(next);
};
