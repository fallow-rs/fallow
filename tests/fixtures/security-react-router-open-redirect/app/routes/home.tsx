import { redirect } from "react-router";

export async function loader() {
  return redirect("/dashboard");
}

export default function Page() {
  return null;
}
