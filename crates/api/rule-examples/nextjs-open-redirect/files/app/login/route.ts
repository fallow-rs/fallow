import { redirect } from "next/navigation";
import type { NextRequest } from "next/server";

export const GET = (request: NextRequest): never => {
  redirect(request.nextUrl.searchParams.get("next") ?? "/");
};
