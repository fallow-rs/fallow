import type { EntryContext } from "react-router";

export default function handleRequest(
  request: Request,
  responseStatusCode: number,
  responseHeaders: Headers,
  routerContext: EntryContext,
) {
  return new Response(String(routerContext.isSpaMode), {
    status: responseStatusCode,
    headers: responseHeaders,
  });
}
