export const streamTimeout = 5_000;

export function handleError(error: unknown) {
  console.error(error);
}

export function handleDataRequest(response: Response) {
  return response;
}

export const strayServerHelper = 1;

export default function handleRequest(request: Request) {
  return new Response(request.url);
}
